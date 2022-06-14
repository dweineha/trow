#[cfg(test)]
mod common;

#[cfg(test)]
mod validation_tests {

    use crate::common;
    use environment::Environment;
    use reqwest::StatusCode;
    use std::fs::{self, File};
    use std::io::{Read, Write};
    use std::process::Child;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    const LYCAON_ADDRESS: &str = "https://trow.test:8443";

    struct TrowInstance {
        pid: Child,
    }

    /// Call out to cargo to start trow.
    /// Seriously considering moving to docker run.
    async fn start_trow() -> TrowInstance {
        let config_file_path = "/tmp/trow-proxy-cfg.json";
        File::create("/tmp/trow-proxy-cfg.json")
            .unwrap()
            .write_all(
                r#"
              default: Deny
              allow:
                - registry-1.docker.io
                - nvcr.io
                - quay.io
                - localhost:8000
                - trow.test
                - k8s.gcr.io
              deny:
                - localhost:8000/secret/shine-box
              "#
                .as_bytes(),
            )
            .unwrap();

        let mut child = Command::new("cargo")
            .arg("run")
            .env_clear()
            .envs(Environment::inherit().compile())
            .arg("--")
            .arg("--names")
            .arg("trow.test")
            .arg("--image-validation-config-file")
            .arg(config_file_path)
            .spawn()
            .expect("failed to start");

        let mut timeout = 100;

        let mut buf = Vec::new();
        File::open("./certs/domain.crt")
            .unwrap()
            .read_to_end(&mut buf)
            .unwrap();
        let cert = reqwest::Certificate::from_pem(&buf).unwrap();
        // get a client builder
        let client = reqwest::Client::builder()
            .add_root_certificate(cert)
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();

        let mut response = client.get(LYCAON_ADDRESS).send().await;
        while timeout > 0 && (response.is_err() || (response.unwrap().status() != StatusCode::OK)) {
            thread::sleep(Duration::from_millis(100));
            response = client.get(LYCAON_ADDRESS).send().await;
            timeout -= 1;
        }
        if timeout == 0 {
            child.kill().unwrap();
            panic!("Failed to start Trow");
        }
        TrowInstance { pid: child }
    }

    impl Drop for TrowInstance {
        fn drop(&mut self) {
            common::kill_gracefully(&self.pid);
        }
    }

    /* Uses a copy of an actual AdmissionReview to test. */
    async fn validate_example(cl: &reqwest::Client) {
        let review = r#"{
  "kind": "AdmissionReview",
  "apiVersion": "admission.k8s.io/v1beta1",
  "request": {
    "uid": "0b4ab323-b607-11e8-a555-42010a8002a3",
    "kind": {
      "group": "",
      "version": "v1",
      "kind": "Pod"
    },
    "resource": {
      "group": "",
      "version": "v1",
      "resource": "pods"
    },
    "namespace": "default",
    "operation": "CREATE",
    "userInfo": {
      "username": "system:serviceaccount:kube-system:replicaset-controller",
      "uid": "fc3f24b4-b5e2-11e8-a555-42010a8002a3",
      "groups": [
        "system:serviceaccounts",
        "system:serviceaccounts:kube-system",
        "system:authenticated"
      ]
    },
    "object": {
      "metadata": {
        "name": "test3-88c6d6597-rll2c",
        "generateName": "test3-88c6d6597-",
        "namespace": "default",
        "uid": "0b4aae46-b607-11e8-a555-42010a8002a3",
        "creationTimestamp": "2018-09-11T21:10:00Z",
        "labels": {
          "pod-template-hash": "447282153",
          "run": "test3"
        },
        "annotations": {
          "kubernetes.io/limit-ranger": "LimitRanger plugin set: cpu request for container test3"
        },
        "ownerReferences": [
          {
            "apiVersion": "networking.k8s.io/v1",
            "kind": "ReplicaSet",
            "name": "test3-88c6d6597",
            "uid": "0b4790c2-b607-11e8-a555-42010a8002a3",
            "controller": true,
            "blockOwnerDeletion": true
          }
        ]
      },
      "spec": {
        "volumes": [
          {
            "name": "default-token-6swbv",
            "secret": {
              "secretName": "default-token-6swbv"
            }
          }
        ],
        "containers": [
          {
            "name": "test3",
            "image": "nginx",
            "resources": {
              "requests": {
                "cpu": "100m"
              }
            },
            "volumeMounts": [
              {
                "name": "default-token-6swbv",
                "readOnly": true,
                "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount"
              }
            ],
            "terminationMessagePath": "/dev/termination-log",
            "terminationMessagePolicy": "File",
            "imagePullPolicy": "Always"
          }
        ],
        "restartPolicy": "Always",
        "terminationGracePeriodSeconds": 30,
        "dnsPolicy": "ClusterFirst",
        "serviceAccountName": "default",
        "serviceAccount": "default",
        "securityContext": {},
        "schedulerName": "default-scheduler",
        "tolerations": [
          {
            "key": "node.kubernetes.io/not-ready",
            "operator": "Exists",
            "effect": "NoExecute",
            "tolerationSeconds": 300
          },
          {
            "key": "node.kubernetes.io/unreachable",
            "operator": "Exists",
            "effect": "NoExecute",
            "tolerationSeconds": 300
          }
        ]
      },
      "status": {
        "phase": "Pending",
        "qosClass": "Burstable"
      }
    },
    "oldObject": null
  }
}"#;

        let resp = cl
            .post(&format!("{}/validate-image", LYCAON_ADDRESS))
            .body(review)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        //should deny by default
        let txt = resp.text().await.unwrap();
        assert!(txt.contains("\"allowed\":false"));
        assert!(txt.contains(
            "Remote image nginx disallowed as not contained in this registry and not in allow list"
        ));
    }

    async fn test_image(cl: &reqwest::Client, image_string: &str, is_allowed: bool) {
        let start = r#"{
  "kind": "AdmissionReview",
  "apiVersion": "admission.k8s.io/v1beta1",
  "request": {
    "uid": "0b4ab323-b607-11e8-a555-42010a8002b4",
    "kind": {
      "group": "",
      "version": "v1",
      "kind": "Pod"
    },
    "resource": {
      "group": "",
      "version": "v1",
      "resource": "pods"
    },
    "namespace": "default",
    "operation": "CREATE",
    "userInfo": {
      "username": "system:serviceaccount:kube-system:replicaset-controller",
      "uid": "fc3f24b4-b5e2-11e8-a555-42010a8002b4",
      "groups": [
        "system:serviceaccounts",
        "system:serviceaccounts:kube-system",
        "system:authenticated"
      ]
    },
    "object": {
      "metadata": {
        "name": "test3-88c6d6597-rll2c",
        "generateName": "test3-88c6d6597-",
        "namespace": "default",
        "uid": "0b4aae46-b607-11e8-a555-42010a8002b4",
        "creationTimestamp": "2018-09-11T21:10:00Z",
        "labels": {
          "pod-template-hash": "447282153",
          "run": "test3"
        }
      },
      "spec": {
        "containers": [
          {
            "name": "test3",
            "image": ""#;
        let end = r#""
          }
        ]
      }
    }
  }
}"#;
        let review = format!("{}{}{}", start, image_string, end);

        let resp = cl
            .post(&format!("{}/validate-image", LYCAON_ADDRESS))
            .body(review)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        //should deny by default
        let txt = resp.text().await.unwrap();
        if is_allowed {
            assert!(txt.contains("\"allowed\":true"));
        } else {
            assert!(txt.contains("\"allowed\":false"));
        }
    }

    #[tokio::test]
    async fn test_runner() {
        //Need to start with empty repo
        fs::remove_dir_all("./data").unwrap_or(());

        let mut buf = Vec::new();
        File::open("./certs/domain.crt")
            .unwrap()
            .read_to_end(&mut buf)
            .unwrap();
        let cert = reqwest::Certificate::from_pem(&buf).unwrap();
        // get a client builder
        let client = reqwest::Client::builder()
            .add_root_certificate(cert)
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();

        //Had issues with stopping and starting trow causing test fails.
        //It might be possible to improve things with a thread_local
        let _trow = start_trow().await;
        validate_example(&client).await;

        // explicitely allowed
        test_image(&client, "trow.test/am/test:tag", true).await;
        test_image(&client, "k8s.gcr.io/metrics-server-amd64:v0.2.1", true).await;
        test_image(&client, "docker.io/amouat/myimage:test", true).await;
        test_image(&client, "http://localhost:8000/hello/world", true).await;

        // explicitely denied
        test_image(&client, "localhost:8000/secret/shine-box", true).await;
        test_image(&client, "http://localhost:8000/secret/shine-box", true).await;
        test_image(&client, "https://localhost:8000/secret/shine-box", true).await;

        // default denied
        test_image(&client, "virus.land.cc/not/suspect", false).await;

        // invalid image ref
        test_image(&client, "http://nope", false).await;
        test_image(&client, "example.com", false).await;
        test_image(&client, "docker.io", false).await;
    }
}
