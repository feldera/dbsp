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
    let hosts = exchange
        .iter()
        .map(|&address| (address, nworkers))
        .collect();
    for (server_addr, exchange_addr) in pool.iter().zip(exchange.iter()) {
        let mut transport = tarpc::serde_transport::tcp::connect(server_addr, Bincode::default);
        transport.config_mut().max_frame_length(usize::MAX);

        let client = CircuitClient::new(client::Config::default(), transport.await?).spawn();
        let layout = Layout::new_multihost(&hosts, *exchange_addr)?;
        println!("{layout:?}");
        let client2 = client.clone();
        join_handles.push(spawn(async move {
            client2.init(context::current(), layout).await
        }));
        clients.push(client);
    }
    for join_handle in join_handles {
        join_handle.await??;
    }

    let path = format!(
        "{}/examples/tutorial/vaccinations.csv",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut reader = Reader::from_path(path)?;
    let mut input_records = reader.deserialize();
    let mut next_client =  (0..clients.len()).cycle();
    loop {
        let mut batches: Vec<_> = (0..clients.len()).map(|_| Vec::new()).collect();
        let mut n = 0;
        while n < 500 {
            let Some(record) = input_records.next() else { break };
            batches[next_client.next().unwrap()].push((record?, 1));
            n += 1;
        }
        if n == 0 {
            break;
        }

        println!("Input {} records:", n);
        let mut joins = Vec::new();
        for (client, input) in clients.iter().zip(batches.into_iter()) {
            let client = client.clone();
            joins.push(spawn(async move {
                client.run(context::current(), input).await.unwrap()
            }));
        }
        for (i, join) in joins.into_iter().enumerate() {
            let output = join.await?;
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
