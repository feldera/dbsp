use std::net::SocketAddr;

use anyhow::{anyhow, Result as AnyResult};
use clap::Parser;
use csv::Reader;
use dbsp::circuit::Layout;
use tarpc::{client, context, tokio_serde::formats::Bincode};

mod service;
use service::*;
use tokio::spawn;

#[derive(Debug, Clone, Parser)]
struct Args {
    /// IP addresses and TCP ports of the pool nodes.
    #[clap(long, required(true))]
    pool: Vec<SocketAddr>,

    /// IP addresses and TCP ports of the pool nodes for exchange purposes.
    #[clap(long, required(true))]
    exchange: Vec<SocketAddr>,
}

#[tokio::main]
async fn main() -> AnyResult<()> {
    let Args { pool, exchange } = Args::parse();
    if pool.len() != exchange.len() {
        Err(anyhow!("--pool and --exchange must be the same length"))?;
    }

    let mut clients: Vec<_> = Vec::new();
    let mut join_handles: Vec<_> = Vec::new();
    let nworkers = 4;
    for (i, &server_addr) in pool.iter().enumerate() {
        let mut transport = tarpc::serde_transport::tcp::connect(server_addr, Bincode::default);
        transport.config_mut().max_frame_length(usize::MAX);

        let client = CircuitClient::new(client::Config::default(), transport.await?).spawn();
        let layout = Layout::new_multihost(
            exchange
                .iter()
                .map(|&address| (address, nworkers))
                .collect(),
            i,
        );
        println!("{layout:?}");
        let client2 = client.clone();
        join_handles.push(spawn(async move {
            client2.init(context::current(), layout).await.unwrap();
        }));
        clients.push(client);
    }
    for join_handle in join_handles {
        join_handle.await?;
    }

    let path = format!(
        "{}/examples/tutorial/vaccinations.csv",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut reader = Reader::from_path(path)?;
    let mut input_records = reader.deserialize();
    for (i, client) in clients.iter().enumerate().cycle() {
        let mut batch = Vec::new();
        while batch.len() < 500 {
            let Some(record) = input_records.next() else { break };
            batch.push((record?, 1));
        }
        if batch.is_empty() {
            break;
        }
        println!("Input {} records to {i}:", batch.len());
        client.append(context::current(), batch).await?;

        let mut joins = Vec::new();
        for client in clients.iter() {
            let client = client.clone();
            joins.push(spawn(async move {
                client.step(context::current()).await.unwrap();
            }));
        }
        for join in joins {
            join.await?;
        }

        for (i, client) in clients.iter().enumerate() {
            let output = client.output(context::current()).await.unwrap();
            output
                .iter()
                .for_each(|(l, VaxMonthly { count, year, month }, w)| {
                    println!("  {i} {l:16} {year}-{month:02} {count:10}: {w:+}")
                });
        }
        println!();
    }

    Ok(())
}
