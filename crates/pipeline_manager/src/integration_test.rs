use std::{
    process::Command,
    time::{self, Duration, Instant},
};

use actix_http::{encoding::Decoder, Payload, StatusCode};
use awc::{http, ClientRequest, ClientResponse};
use aws_sdk_cognitoidentityprovider::config::Region;
use serde_json::{json, Value};
use serial_test::serial;
use tempfile::TempDir;
use tokio::sync::OnceCell;

use crate::{
    compiler::Compiler,
    config::{CompilerConfig, DatabaseConfig, ManagerConfig},
};

const TEST_DBSP_URL_VAR: &str = "TEST_DBSP_URL";
const TEST_DBSP_DEFAULT_PORT: u16 = 8089;

// Used if we are testing against a local DBSP instance
// whose lifecycle is managed by this test file
static LOCAL_DBSP_INSTANCE: OnceCell<TempDir> = OnceCell::const_new();

async fn initialize_local_dbsp_instance() -> TempDir {
    println!("Performing one time initialization for integration tests.");
    println!("Initializing a postgres container");
    let _output = Command::new("docker")
        .args([
            "compose",
            "-f",
            "../../deploy/docker-compose.yml",
            "-f",
            "../../deploy/docker-compose-dev.yml",
            "up",
            "--renew-anon-volumes",
            "--force-recreate",
            "-d",
            "db", // run only the DB service
        ])
        .output()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(5000)).await;
    let tmp_dir = TempDir::new().unwrap();
    let workdir = tmp_dir.path().to_str().unwrap();
    let database_config = DatabaseConfig {
        db_connection_string: "postgresql://postgres:postgres@localhost:6666".to_owned(),
        initial_sql: None,
    };
    let manager_config = ManagerConfig {
        port: TEST_DBSP_DEFAULT_PORT,
        bind_address: "0.0.0.0".to_owned(),
        logfile: None,
        manager_working_directory: workdir.to_owned(),
        unix_daemon: false,
        use_auth: false,
        dev_mode: false,
        dump_openapi: false,
        config_file: None,
    }
    .canonicalize()
    .unwrap();
    let compiler_config = CompilerConfig {
        compiler_working_directory: workdir.to_owned(),
        sql_compiler_home: "../../sql-to-dbsp-compiler".to_owned(),
        dbsp_override_path: Some("../../".to_owned()),
        debug: false,
        precompile: true,
    }
    .canonicalize()
    .unwrap();
    println!("Using ManagerConfig: {:?}", manager_config);
    println!("Issuing Compiler::precompile_dependencies(). This will be slow.");
    Compiler::precompile_dependencies(&compiler_config)
        .await
        .unwrap();
    println!("Completed Compiler::precompile_dependencies().");

    // We can't use tokio::spawn because super::run() creates its own Tokio Runtime
    let _ = std::thread::spawn(|| {
        super::run(database_config, manager_config, compiler_config).unwrap();
    });
    tokio::time::sleep(Duration::from_millis(1000)).await;
    tmp_dir
}

struct TestConfig {
    dbsp_url: String,
    client: awc::Client,
    bearer_token: Option<String>,
}

impl TestConfig {
    fn endpoint_url<S: AsRef<str>>(&self, endpoint: S) -> String {
        format!("{}{}", self.dbsp_url, endpoint.as_ref())
    }

    async fn cleanup(&self) {
        let config = self;

        // Cleanup pipelines..
        let mut req = config.get("/v0/pipelines").await;
        let pipelines: Value = req.json().await.unwrap();
        for pipeline in pipelines.as_array().unwrap() {
            let id = pipeline["pipeline_id"].as_str().unwrap();
            let req = config.delete(format!("/v0/pipelines/{}", id)).await;
            assert_eq!(StatusCode::OK, req.status())
        }

        // .. programs
        let mut req = config.get("/v0/programs").await;
        let programs: Value = req.json().await.unwrap();
        for program in programs.as_array().unwrap() {
            let id = program["program_id"].as_str().unwrap();
            let req = config.delete(format!("/v0/programs/{}", id)).await;
            assert_eq!(StatusCode::OK, req.status())
        }

        // .. connectors
        let mut req = config.get("/v0/connectors").await;
        let programs: Value = req.json().await.unwrap();
        for program in programs.as_array().unwrap() {
            let id = program["connector_id"].as_str().unwrap();
            let req = config.delete(format!("/v0/connectors/{}", id)).await;
            assert_eq!(StatusCode::OK, req.status())
        }
    }

    async fn get<S: AsRef<str>>(&self, endpoint: S) -> ClientResponse<Decoder<Payload>> {
        self.maybe_attach_bearer_token(self.client.get(self.endpoint_url(endpoint)))
            .send()
            .await
            .unwrap()
    }

    fn maybe_attach_bearer_token(&self, req: ClientRequest) -> ClientRequest {
        match &self.bearer_token {
            Some(token) => {
                req.insert_header((http::header::AUTHORIZATION, format!("Bearer {}", token)))
            }
            None => req,
        }
    }

    async fn post_no_body<S: AsRef<str>>(&self, endpoint: S) -> ClientResponse<Decoder<Payload>> {
        self.maybe_attach_bearer_token(self.client.post(self.endpoint_url(endpoint)))
            .send()
            .await
            .unwrap()
    }

    async fn post<S: AsRef<str>>(
        &self,
        endpoint: S,
        json: &Value,
    ) -> ClientResponse<Decoder<Payload>> {
        self.maybe_attach_bearer_token(self.client.post(self.endpoint_url(endpoint)))
            .send_json(&json)
            .await
            .unwrap()
    }

    async fn delete<S: AsRef<str>>(&self, endpoint: S) -> ClientResponse<Decoder<Payload>> {
        self.maybe_attach_bearer_token(self.client.delete(self.endpoint_url(endpoint)))
            .send()
            .await
            .unwrap()
    }

    async fn compile(&self, program_id: &str, version: i64) {
        let compilation_request = json!({
            "program_id":  program_id,
            "version": version
        });

        let resp = self
            .post("/v0/programs/compile", &compilation_request)
            .await;
        assert_eq!(StatusCode::ACCEPTED, resp.status());

        let now = Instant::now();
        loop {
            println!("Waiting for compilation");
            std::thread::sleep(time::Duration::from_secs(1));
            if now.elapsed().as_secs() > 100 {
                panic!("Compilation timeout");
            }
            let mut resp = self.get(format!("/v0/program?id={}", program_id)).await;
            let val: Value = resp.json().await.unwrap();
            let status = val["status"].as_str().unwrap();
            if status != "CompilingSql" && status != "CompilingRust" && status != "Pending" {
                if status == "Success" {
                    break;
                } else {
                    panic!("Compilation failed with status {}", status);
                }
            }
        }
    }
}

async fn bearer_token() -> Option<String> {
    let client_id = std::env::var("TEST_CLIENT_ID");
    match client_id {
        Ok(client_id) => {
            let test_user = std::env::var("TEST_USER")
                .expect("If TEST_CLIENT_ID is set, TEST_USER should be as well");
            let test_password = std::env::var("TEST_PASSWORD")
                .expect("If TEST_CLIENT_ID is set, TEST_PASSWORD should be as well");
            let test_region = std::env::var("TEST_REGION")
                .expect("If TEST_CLIENT_ID is set, TEST_REGION should be as well");
            let config = aws_config::from_env()
                .region(Region::new(test_region))
                .load()
                .await;
            let cognito_idp = aws_sdk_cognitoidentityprovider::Client::new(&config);
            let res = cognito_idp
                .initiate_auth()
                .set_client_id(Some(client_id))
                .set_auth_flow(Some(
                    aws_sdk_cognitoidentityprovider::types::AuthFlowType::UserPasswordAuth,
                ))
                .auth_parameters("USERNAME", test_user)
                .auth_parameters("PASSWORD", test_password)
                .send()
                .await
                .unwrap();
            Some(
                res.authentication_result()
                    .unwrap()
                    .access_token()
                    .unwrap()
                    .to_string(),
            )
        }
        Err(e) => {
            println!(
                "TEST_CLIENT_ID is unset (reason: {}). Test will not use authentication",
                e
            );
            None
        }
    }
}

async fn setup() -> TestConfig {
    let dbsp_url = match std::env::var(TEST_DBSP_URL_VAR) {
        Ok(val) => {
            println!("Running integration test against TEST_DBSP_URL: {}", val);
            val
        }
        Err(e) => {
            println!(
                "TEST_DBSP_URL is unset (reason: {}). Running integration test against: localhost:{}", e, TEST_DBSP_DEFAULT_PORT
            );
            LOCAL_DBSP_INSTANCE
                .get_or_init(initialize_local_dbsp_instance)
                .await;
            format!("http://localhost:{}", TEST_DBSP_DEFAULT_PORT).to_owned()
        }
    };
    let client = awc::Client::default();
    let bearer_token = bearer_token().await;
    let config = TestConfig {
        dbsp_url,
        client,
        bearer_token,
    };
    config.cleanup().await;
    config
}

#[actix_web::test]
#[serial]
async fn lists_at_initialization_are_empty() {
    let config = setup().await;
    for endpoint in vec!["/v0/pipelines", "/v0/programs", "/v0/connectors"] {
        let mut req = config.get(endpoint).await;
        let ret: Value = req.json().await.unwrap();
        assert_eq!(ret, json!([]));
    }
}

#[actix_web::test]
#[serial]
async fn program_create_compile_delete() {
    let config = setup().await;
    let program_request = json!({
        "name":  "test",
        "description": "desc",
        "code": "create table t1(c1 integer);"
    });
    let mut req = config.post("/v0/programs", &program_request).await;
    assert_eq!(StatusCode::CREATED, req.status());
    let resp: Value = req.json().await.unwrap();
    let id = resp["program_id"].as_str().unwrap();
    let version = resp["version"].as_i64().unwrap();
    config.compile(id, version).await;
    let resp = config.delete(format!("/v0/programs/{}", id)).await;
    assert_eq!(StatusCode::OK, resp.status());
    let resp = config.get(format!("/v0/program?id={}", id).as_str()).await;
    assert_eq!(StatusCode::NOT_FOUND, resp.status());
}

#[actix_web::test]
#[serial]
async fn program_create_twice() {
    let config = setup().await;
    let program_request = json!({
        "name":  "test",
        "description": "desc",
        "code": "create table t1(c1 integer);"
    });
    let req = config.post("/v0/programs", &program_request).await;
    assert_eq!(StatusCode::CREATED, req.status());

    // Same name, different desc/program.
    let program_request = json!({
        "name":  "test",
        "description": "desc1",
        "code": "create table t2(c2 integer);"
    });
    let req = config.post("/v0/programs", &program_request).await;
    assert_eq!(StatusCode::CONFLICT, req.status());
}

#[actix_web::test]
#[serial]
async fn deploy_pipeline() {
    let config = setup().await;
    let program_request = json!({
        "name":  "test",
        "description": "desc",
        "code": "create table t1(c1 integer);"
    });
    let mut req = config.post("/v0/programs", &program_request).await;
    assert_eq!(StatusCode::CREATED, req.status());
    let resp: Value = req.json().await.unwrap();
    let id = resp["program_id"].as_str().unwrap();
    let version = resp["version"].as_i64().unwrap();
    config.compile(id, version).await;

    let pipeline_request = json!({
        "name":  "test",
        "description": "desc",
        "program_id": Some(id.to_string()),
        "config": "",
        "connectors": null
    });
    let mut req = config.post("/v0/pipelines", &pipeline_request).await;
    let resp: Value = req.json().await.unwrap();
    let id = resp["pipeline_id"].as_str().unwrap();

    //
    // TODO: We should not bubble a connection refused error. Instead,
    // we should return an error about the pipeline not running.
    // Tracking issue: https://github.com/feldera/dbsp/issues/343
    //
    // Start the pipeline before it has been deployed
    // let mut req = config
    //     .post_no_body(format!("/v0/pipelines/{}/start", id))
    //     .await;
    // let resp: Value = req.json().await.unwrap();
    // assert_eq!("Pipeline is not deployed", resp.as_str().unwrap());

    // Deploy the pipeline
    let mut req = config
        .post_no_body(format!("/v0/pipelines/{}/deploy", id))
        .await;
    let resp: Value = req.json().await.unwrap();
    assert_eq!("Pipeline successfully deployed.", resp.as_str().unwrap());

    // Pause a pipeline before it is started
    let mut req = config
        .post_no_body(format!("/v0/pipelines/{}/pause", id))
        .await;
    let resp: Value = req.json().await.unwrap();
    assert_eq!("Pipeline paused", resp.as_str().unwrap());

    // Start the pipeline
    let mut req = config
        .post_no_body(format!("/v0/pipelines/{}/start", id))
        .await;
    let resp: Value = req.json().await.unwrap();
    assert_eq!("The pipeline is running", resp.as_str().unwrap());

    // Pause a pipeline after it is started
    let mut req = config
        .post_no_body(format!("/v0/pipelines/{}/pause", id))
        .await;
    let resp: Value = req.json().await.unwrap();
    assert_eq!("Pipeline paused", resp.as_str().unwrap());

    // Start the pipeline
    let mut req = config
        .post_no_body(format!("/v0/pipelines/{}/start", id))
        .await;
    let resp: Value = req.json().await.unwrap();
    assert_eq!("The pipeline is running", resp.as_str().unwrap());

    // TODO: Deploying again currently creates another instance, which
    // might not be correct behavior

    // let mut req = config
    //     .post_no_body(format!("/v0/pipelines/{}/deploy", id))
    //     .await;
    // let resp: Value = req.json().await.unwrap();
    // assert_eq!("Pipeline successfully deployed.", resp.as_str().unwrap());

    // Shutdown the pipeline
    let mut req = config
        .post_no_body(format!("/v0/pipelines/{}/shutdown", id))
        .await;
    let resp: Value = req.json().await.unwrap();
    assert_eq!("Pipeline successfully terminated.", resp.as_str().unwrap());

    //
    // TODO: We should not bubble a connection refused error. Instead,
    // we should return an error about the pipeline not running.
    // Tracking issue: https://github.com/feldera/dbsp/issues/343
    //
    // let mut req = config
    //     .post_no_body(format!("/v0/pipelines/{}/pause", id))
    //     .await;
    // let resp: Value = req.json().await.unwrap();
    // assert_eq!("Pipeline is not deployed", resp.as_str().unwrap());
}
