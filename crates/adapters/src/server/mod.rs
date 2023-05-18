use crate::{
    Catalog, Controller, ControllerError, HttpInputTransport, HttpOutputTransport, PipelineConfig,
};
use actix_web::{
    dev::{Server, ServiceFactory, ServiceRequest},
    get,
    middleware::Logger,
    rt, web,
    web::Data as WebData,
    App, Error as ActixError, HttpRequest, HttpResponse, HttpServer, Responder,
};
use actix_web_static_files::ResourceFiles;
use anyhow::{Error as AnyError, Result as AnyResult};
use clap::Parser;
use colored::Colorize;
use dbsp::DBSPHandle;
use env_logger::Env;
use log::{error, info};
use serde::Serialize;
use std::io::Write;
use std::{net::TcpListener, sync::Mutex};
use tokio::{
    spawn,
    sync::mpsc::{channel, Receiver, Sender},
};
mod prometheus;

use self::prometheus::PrometheusMetrics;

struct ServerState {
    metadata: String,
    controller: Mutex<Option<Controller>>,
    prometheus: PrometheusMetrics,
    /// Channel used to send a `kill` command to
    /// the self-destruct task when shutting down
    /// the server.
    terminate_sender: Option<Sender<()>>,
}

impl ServerState {
    fn new(
        controller: Controller,
        prometheus: PrometheusMetrics,
        meta: String,
        terminate_sender: Option<Sender<()>>,
    ) -> Self {
        Self {
            metadata: meta,
            controller: Mutex::new(Some(controller)),
            prometheus,
            terminate_sender,
        }
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    message: String,
}

impl ErrorResponse {
    pub(crate) fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Pipeline configuration YAML file
    #[arg(short, long)]
    config_file: String,

    /// Pipeline metadata JSON file
    #[arg(short, long)]
    metadata_file: Option<String>,

    /// Run the server on this port if it is available. If the port is in
    /// use or no default port is specified, an unused TCP port is allocated
    /// automatically
    #[arg(short = 'p', long)]
    default_port: Option<u16>,
}

// This file indicates the port used by the server
pub const SERVER_PORT_FILE: &str = "port";

/// Server main function.
///
/// This function is intended to be invoked from the code generated by,
/// e.g., the SQL compiler.  It performs the following steps needed to start
/// a circuit server:
///
/// * Setup logging.
/// * Parse command line arguments.
/// * Start the server.
///
/// # Arguments
///
/// * `circuit_factory` - a function that creates a circuit and builds an
///   input/output stream
/// catalog.
pub fn server_main<F>(circuit_factory: &F) -> AnyResult<()>
where
    F: Fn(usize) -> (DBSPHandle, Catalog),
{
    let args = Args::try_parse()?;
    let yaml_config = std::fs::read(&args.config_file)?;
    let yaml_config = String::from_utf8(yaml_config)?;
    let config: PipelineConfig = serde_yaml::from_str(yaml_config.as_str()).map_err(|e| {
        let err = format!("error parsing pipeline configuration: {e}");
        error!("{err}");
        AnyError::msg(err)
    })?;

    // Create env logger.
    let pipeline_name = format!("[{}]", config.name.clone().unwrap_or_default()).cyan();
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format(move |buf, record| {
            let t = chrono::Utc::now();
            let t = format!("{}", t.format("%Y-%m-%d %H:%M:%S"));
            writeln!(
                buf,
                "{t} {} {pipeline_name} {}",
                buf.default_styled_level(record.level()),
                record.args()
            )
        })
        .init();

    let meta = match args.metadata_file {
        None => String::new(),
        Some(metadata_file) => {
            let meta = std::fs::read(metadata_file)?;
            String::from_utf8(meta)?
        }
    };
    run_server(circuit_factory, &config, meta, args.default_port).map_err(|e| {
        error!("{e}");
        e
    })
}

pub fn run_server<F>(
    circuit_factory: &F,
    config: &PipelineConfig,
    meta: String,
    default_port: Option<u16>,
) -> AnyResult<()>
where
    F: Fn(usize) -> (DBSPHandle, Catalog),
{
    // NOTE: The pipeline manager monitors pipeline log for one of the following
    // messages ("Failed to create pipeline..." or "Started HTTP server...").
    // If you change these messages, make sure to make a corresponding change to
    // `runner.rs`.
    let (port, server, mut terminate_receiver) =
        create_server(circuit_factory, config, meta, default_port)
            .map_err(|e| AnyError::msg(format!("Failed to create pipeline: {e}")))?;

    std::fs::write(SERVER_PORT_FILE, format!("{}\n", port))?;
    info!("Started HTTP server on port {port}");

    rt::System::new().block_on(async {
        // Spawn a task that will shutdown the server on `/kill`.
        let server_handle = server.handle();
        spawn(async move {
            terminate_receiver.recv().await;
            server_handle.stop(true).await
        });

        server.await
    })?;
    Ok(())
}

pub fn create_server<F>(
    circuit_factory: &F,
    config: &PipelineConfig,
    meta: String,
    default_port: Option<u16>,
) -> AnyResult<(u16, Server, Receiver<()>)>
where
    F: Fn(usize) -> (DBSPHandle, Catalog),
{
    let (circuit, catalog) = circuit_factory(config.global.workers as usize);

    let controller = Controller::with_config(
        circuit,
        catalog,
        config,
        Box::new(|e| error!("{e}")) as Box<dyn Fn(ControllerError) + Send + Sync>,
    )?;

    let prometheus = PrometheusMetrics::new(&controller)
        .map_err(|e| AnyError::msg(format!("failed to initialize Prometheus metrics: {e}")))?;

    let listener = match default_port {
        Some(port) => TcpListener::bind(("127.0.0.1", port))
            .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))?,
        None => TcpListener::bind(("127.0.0.1", 0))?,
    };

    let port = listener.local_addr()?.port();

    let (terminate_sender, terminate_receiver) = channel(1);
    let state = WebData::new(ServerState::new(
        controller,
        prometheus,
        meta,
        Some(terminate_sender),
    ));
    let server =
        HttpServer::new(move || build_app(App::new().wrap(Logger::default()), state.clone()))
            .workers(1)
            .listen(listener)?
            .run();

    Ok((port, server, terminate_receiver))
}

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

fn build_app<T>(app: App<T>, state: WebData<ServerState>) -> App<T>
where
    T: ServiceFactory<ServiceRequest, Config = (), Error = ActixError, InitError = ()>,
{
    let generated = generate();

    let index_data = match generated.get("index.html") {
        None => "<html><head><title>DBSP server</title></head></html>"
            .as_bytes()
            .to_owned(),
        Some(resource) => resource.data.to_owned(),
    };

    app.app_data(state)
        .route(
            "/",
            web::get().to(move || {
                let index_data = index_data.clone();
                async { HttpResponse::Ok().body(index_data) }
            }),
        )
        .service(ResourceFiles::new("/static", generated))
        .service(start)
        .service(pause)
        .service(shutdown)
        .service(status)
        .service(metrics)
        .service(metadata)
        .service(dump_profile)
        .service(input_endpoint)
        .service(output_endpoint)
}

#[get("/start")]
async fn start(state: WebData<ServerState>) -> impl Responder {
    match &*state.controller.lock().unwrap() {
        Some(controller) => {
            controller.start();
            HttpResponse::Ok().json("The pipeline is running")
        }
        None => {
            HttpResponse::Conflict().json(&ErrorResponse::new("The pipeline has been terminated"))
        }
    }
}

#[get("/pause")]
async fn pause(state: WebData<ServerState>) -> impl Responder {
    match &*state.controller.lock().unwrap() {
        Some(controller) => {
            controller.pause();
            HttpResponse::Ok().json("Pipeline paused")
        }
        None => {
            HttpResponse::Conflict().json(&ErrorResponse::new("The pipeline has been terminated"))
        }
    }
}

#[get("/status")]
async fn status(state: WebData<ServerState>) -> impl Responder {
    match &*state.controller.lock().unwrap() {
        Some(controller) => {
            let json_string = serde_json::to_string(controller.status()).unwrap();
            HttpResponse::Ok()
                .content_type(mime::APPLICATION_JSON)
                .body(json_string)
        }
        None => {
            HttpResponse::Conflict().json(&ErrorResponse::new("The pipeline has been terminated"))
        }
    }
}

/// This endpoint is invoked by the Prometheus server.
#[get("/metrics")]
async fn metrics(state: WebData<ServerState>) -> impl Responder {
    match &*state.controller.lock().unwrap() {
        Some(controller) => match state.prometheus.metrics(controller) {
            Ok(metrics) => HttpResponse::Ok()
                .content_type(mime::TEXT_PLAIN)
                .body(metrics),
            Err(e) => {
                HttpResponse::InternalServerError().body(format!("Error retrieving metrics: {e}"))
            }
        },
        None => HttpResponse::Conflict().body("The pipeline has been terminated"),
    }
}

#[get("/metadata")]
async fn metadata(state: WebData<ServerState>) -> impl Responder {
    HttpResponse::Ok()
        .content_type(mime::APPLICATION_JSON)
        .body(state.metadata.clone())
}

#[get("/dump_profile")]
async fn dump_profile(state: WebData<ServerState>) -> impl Responder {
    match &*state.controller.lock().unwrap() {
        Some(controller) => {
            controller.dump_profile();
            HttpResponse::Ok().json("Profile dump initiated")
        }
        None => {
            HttpResponse::Conflict().json(&ErrorResponse::new("The pipeline has been terminated"))
        }
    }
}

#[get("/shutdown")]
async fn shutdown(state: WebData<ServerState>) -> impl Responder {
    let controller = state.controller.lock().unwrap().take();
    if let Some(controller) = controller {
        match controller.stop() {
            Ok(()) => {
                if let Some(sender) = &state.terminate_sender {
                    let _ = sender.send(()).await;
                }
                HttpResponse::Ok().json("Pipeline terminated")
            }
            Err(e) => HttpResponse::InternalServerError().json(&ErrorResponse::new(&format!(
                "Failed to terminate the pipeline: {e}"
            ))),
        }
    } else {
        HttpResponse::Ok().json("Pipeline already terminated")
    }
}

#[get("/input_endpoint/{endpoint_name}")]
async fn input_endpoint(req: HttpRequest, stream: web::Payload) -> impl Responder {
    match req.match_info().get("endpoint_name") {
        None => HttpResponse::BadRequest().body("Missing endpoint name argument"),
        Some(endpoint_name) => {
            HttpInputTransport::get_endpoint_websocket(endpoint_name, &req, stream).unwrap_or_else(
                |e| {
                    HttpResponse::InternalServerError().json(&ErrorResponse::new(&format!(
                        "Failed to establish connection to input HTTP endpoint: {e}"
                    )))
                },
            )
        }
    }
}

#[get("/output_endpoint/{endpoint_name}")]
async fn output_endpoint(req: HttpRequest, stream: web::Payload) -> impl Responder {
    match req.match_info().get("endpoint_name") {
        None => HttpResponse::BadRequest().body("Missing endpoint name argument"),
        Some(endpoint_name) => {
            HttpOutputTransport::get_endpoint_websocket(endpoint_name, &req, stream).unwrap_or_else(
                |e| {
                    HttpResponse::InternalServerError().json(&ErrorResponse::new(&format!(
                        "Failed to establish connection to output HTTP endpoint: {e}"
                    )))
                },
            )
        }
    }
}

#[cfg(test)]
#[cfg(feature = "with-kafka")]
#[cfg(feature = "server")]
mod test_with_kafka {
    use super::{build_app, PrometheusMetrics, ServerState};
    use crate::{
        test::{
            generate_test_batches,
            kafka::{BufferConsumer, KafkaResources, TestProducer},
            test_circuit, wait,
            websocket::{TestWsReceiver, TestWsSender},
            TEST_LOGGER,
        },
        Controller, ControllerError, PipelineConfig,
    };
    use actix_http::ws::{Frame as WsFrame, Message as WsMessage};
    use actix_web::{http::StatusCode, middleware::Logger, web::Data as WebData, App};
    use bytes::Bytes;
    use bytestring::ByteString;
    use crossbeam::queue::SegQueue;
    use futures::{SinkExt, StreamExt};
    use log::{error, LevelFilter};
    use proptest::{
        strategy::{Strategy, ValueTree},
        test_runner::TestRunner,
    };
    use std::{pin::Pin, sync::Arc, thread::sleep, time::Duration};

    #[actix_web::test]
    async fn test_server() {
        // We cannot use proptest macros in `async` context, so generate
        // some random data manually.
        let mut runner = TestRunner::default();
        let data = generate_test_batches(100, 1000)
            .new_tree(&mut runner)
            .unwrap()
            .current();

        let _ = log::set_logger(&TEST_LOGGER);
        log::set_max_level(LevelFilter::Debug);

        // Create topics.
        let kafka_resources = KafkaResources::create_topics(&[
            ("test_server_input_topic", 1),
            ("test_server_output_topic", 1),
        ]);

        // Create buffer consumer
        let buffer_consumer = BufferConsumer::new("test_server_output_topic");

        // Config string
        let config_str = r#"
name: test
inputs:
    test_input1:
        stream: test_input1
        transport:
            name: kafka
            config:
                bootstrap.servers: "localhost"
                auto.offset.reset: "earliest"
                topics: [test_server_input_topic]
                log_level: debug
        format:
            name: csv
    test_input_http:
        stream: test_input1
        transport:
            name: http
        format:
            name: csv
outputs:
    test_output2:
        stream: test_output1
        transport:
            name: kafka
            config:
                bootstrap.servers: "localhost"
                topic: test_server_output_topic
                max_inflight_messages: 0
        format:
            name: csv
    test_output_http:
        stream: test_output1
        transport:
            name: http
        format:
            name: csv
"#;

        // Create circuit
        println!("Creating circuit");
        let (circuit, catalog) = test_circuit(4);

        let errors = Arc::new(SegQueue::new());
        let errors_clone = errors.clone();

        let config: PipelineConfig = serde_yaml::from_str(config_str).unwrap();
        let controller = Controller::with_config(
            circuit,
            catalog,
            &config,
            Box::new(move |e| {
                error!("{e}");
                errors_clone.push(e);
            }) as Box<dyn Fn(ControllerError) + Send + Sync>,
        )
        .unwrap();

        // Create service
        println!("Creating HTTP server");

        let prometheus = PrometheusMetrics::new(&controller).unwrap();
        let state = WebData::new(ServerState::new(
            controller,
            prometheus,
            "metadata".to_string(),
            None,
        ));
        let mut server =
            actix_test::start(move || build_app(App::new().wrap(Logger::default()), state.clone()));

        // Write data to Kafka.
        println!("Send test data");
        let producer = TestProducer::new();
        producer.send_to_topic(&data, "test_server_input_topic");

        sleep(Duration::from_millis(2000));
        assert!(buffer_consumer.is_empty());

        // Start command; wait for data.
        println!("/start");
        let resp = server.get("/start").send().await.unwrap();
        assert!(resp.status().is_success());

        buffer_consumer.wait_for_output_unordered(&data);
        buffer_consumer.clear();

        println!("/status");
        let resp = server.get("/status").send().await.unwrap();
        assert!(resp.status().is_success());

        println!("/metadata");
        let resp = server.get("/metadata").send().await.unwrap();
        assert!(resp.status().is_success());

        // Pause command; send more data, receive none.
        println!("/pause");
        let resp = server.get("/pause").send().await.unwrap();
        assert!(resp.status().is_success());
        sleep(Duration::from_millis(1000));

        producer.send_to_topic(&data, "test_server_input_topic");
        sleep(Duration::from_millis(2000));
        assert_eq!(buffer_consumer.len(), 0);

        // Start; wait for data
        println!("/start");
        let resp = server.get("/start").send().await.unwrap();
        assert!(resp.status().is_success());

        buffer_consumer.wait_for_output_unordered(&data);
        buffer_consumer.clear();

        println!("Testing invalid input");
        producer.send_string("invalid\n", "test_server_input_topic");
        wait(|| errors.len() == 1, None);

        println!("Connecting to HTTP input endpoint");
        let mut ws1 = server
            .ws_at("/input_endpoint/test_input_http")
            .await
            .unwrap();
        ws1.send(WsMessage::Text(ByteString::from_static("state")))
            .await
            .unwrap();
        assert_eq!(
            ws1.next().await.unwrap().unwrap(),
            WsFrame::Text(Bytes::from("running"))
        );

        let mut ws2 = server
            .ws_at("/input_endpoint/test_input_http")
            .await
            .unwrap();
        ws2.send(WsMessage::Text(ByteString::from_static("state")))
            .await
            .unwrap();
        assert_eq!(
            ws2.next().await.unwrap().unwrap(),
            WsFrame::Text(Bytes::from("running"))
        );

        println!("Connecting to HTTP output endpoint");
        let mut outws1 = server
            .ws_at("/output_endpoint/test_output_http")
            .await
            .unwrap();
        outws1
            .send(WsMessage::Ping(Bytes::from("ping")))
            .await
            .unwrap();
        assert_eq!(
            outws1.next().await.unwrap().unwrap(),
            WsFrame::Pong(Bytes::from("ping"))
        );

        let mut outws2 = server
            .ws_at("/output_endpoint/test_output_http")
            .await
            .unwrap();
        outws2
            .send(WsMessage::Ping(Bytes::from("ping")))
            .await
            .unwrap();
        assert_eq!(
            outws2.next().await.unwrap().unwrap(),
            WsFrame::Pong(Bytes::from("ping"))
        );

        println!("Websocket test: whole messages");
        TestWsSender::send_to_websocket(Pin::new(&mut ws1), &data).await;

        buffer_consumer.wait_for_output_unordered(&data);
        buffer_consumer.clear();

        TestWsReceiver::wait_for_output_unordered(Pin::new(&mut outws1), &data).await;
        TestWsReceiver::wait_for_output_unordered(Pin::new(&mut outws2), &data).await;

        TestWsSender::send_to_websocket(Pin::new(&mut ws2), &data).await;

        buffer_consumer.wait_for_output_unordered(&data);
        buffer_consumer.clear();

        TestWsReceiver::wait_for_output_unordered(Pin::new(&mut outws1), &data).await;
        TestWsReceiver::wait_for_output_unordered(Pin::new(&mut outws2), &data).await;

        println!("Websocket test: continuations");
        TestWsSender::send_to_websocket_continuations(Pin::new(&mut ws1), &data).await;

        buffer_consumer.wait_for_output_unordered(&data);
        buffer_consumer.clear();

        TestWsSender::send_to_websocket_continuations(Pin::new(&mut ws2), &data).await;

        buffer_consumer.wait_for_output_unordered(&data);
        buffer_consumer.clear();

        println!("/pause");
        let resp = server.get("/pause").send().await.unwrap();
        assert!(resp.status().is_success());
        sleep(Duration::from_millis(1000));

        // Make sure the websocket gets status update.
        assert_eq!(
            ws1.next().await.unwrap().unwrap(),
            WsFrame::Text(Bytes::from("paused"))
        );

        // Shutdown
        println!("/shutdown");
        let resp = server.get("/shutdown").send().await.unwrap();
        // println!("Response: {resp:?}");
        assert!(resp.status().is_success());

        // Start after shutdown must fail.
        println!("/start");
        let resp = server.get("/start").send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        drop(buffer_consumer);
        drop(kafka_resources);
    }
}
