use anyhow::{Context, Result};
use arrow::datatypes::{DataType, Field, Schema, ToByteSlice};
use fastembed::TextEmbedding;
use lancedb::Connection;
use rayon::iter::{ParallelBridge, ParallelExtend, ParallelIterator};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::env::args;
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::{fs::read_dir, io, path::PathBuf, str::FromStr};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, Interest};
use tokio::net::unix::SocketAddr;
use tokio::net::{UnixListener, UnixStream};
use tokio::select;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, RwLock};
use tracing::{error, info, warn};
use vs_core::{Message, TrackArgs};

#[derive(Serialize, Deserialize, Default)]
struct Config {
    tracked: HashSet<PathBuf>,
}

impl Config {
    fn save(&self) {
        let message = match serde_json::to_string(self) {
            Ok(message) => message,
            Err(e) => {
                error!(error=?e, "Unable to serialize config");
                return;
            }
        };

        let path = Path::new("/tmp/vs.config");
        let mut opts = OpenOptions::new();
        opts.write(true).truncate(true);
        match opts.open(path) {
            Ok(mut file) => {
                if let Err(e) = file.write(message.into_bytes().to_byte_slice()) {
                    error!(error=?e, "Unable to write config to vs.config");
                }
            }
            Err(e) => {
                error!(error=?e, "Unable to open vs.config for saving");
            }
        }
    }
}

fn crawl_directory<T: Send + Sync>(
    dir: PathBuf,
    args: TrackArgs,
    f: fn(&PathBuf, &TrackArgs) -> Option<T>,
) -> io::Result<Vec<T>> {
    let mut out = match f(&dir, &args) {
        Some(t) => vec![t],
        None => Vec::new(),
    };
    if dir.is_dir() && args.recursive && (!dir.is_symlink() || args.follow_symlinks) {
        out.extend({
            let entries = read_dir(dir)?;
            let results = entries
                .par_bridge()
                .filter_map(Result::ok)
                .filter_map(|p| crawl_directory(p.path(), args.clone(), f).ok())
                .flatten()
                .collect::<Vec<T>>();
            results
        });
    }
    Ok(out)
}

async fn handle_connection(
    mut stream: UnixStream,
    tx: mpsc::Sender<(Message, oneshot::Sender<Message>)>, // Send messages to daemon
    addr: SocketAddr,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!(addr=?addr, "Established Connection");
    let mut outbound = VecDeque::new();
    stream.split();
    loop {
        let ready = stream
            .ready(Interest::READABLE | Interest::WRITABLE)
            .await?;

        if ready.is_readable() {
            let mut message = String::new();
            let _ = stream.read_to_string(&mut message);
            let message: Message = serde_json::from_str(&message)
                .with_context(|| "Failed to parse message over socket")?;
            let (otx, mut orx) = oneshot::channel();
            tx.send((message, otx)).await?;
            outbound.push_front(orx.try_recv().with_context(|| "Failed to read callback")?)
        }

        if ready.is_writable() && !outbound.is_empty() {
            let message = serde_json::to_string(&outbound.pop_back())
                .with_context(|| "Failed to serialize message")?;
            stream
                .write_all(message.into_bytes().to_byte_slice())
                .await
                .with_context(|| "Unable to write to socket")?;
            stream
                .flush()
                .await
                .with_context(|| "Unable to flush socket")?;
        }
    }
}

async fn get_config() -> Result<Config, Box<dyn Error + Send + Sync>> {
    let path = Path::new("/tmp/vs.config");
    Ok(if path.exists() && path.is_file() {
        let mut file =
            File::open(path).with_context(|| "Failed to load config file, despite it existing")?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .with_context(|| "Failed to read config file")?;
        match serde_json::from_str::<Config>(&contents) {
            Ok(config) => config,
            Err(e) => {
                warn!(error=?e, "Failed to parse existing config");
                Config::default()
            }
        }
    } else {
        Config::default()
    })
}

fn track(path: &PathBuf, args: &TrackArgs) -> Option<PathBuf> {
    // Embed the path and save it to the database
    if !path.exists() {
        return None;
    }

    if path.is_dir() && !args.recursive {
        // Embed the name
    }
    if path.is_file() {
        // Embed the name + contents
    }

    // Return the path for tracking
    Some(path.clone())
}

async fn maintainer(
    mut rx: mpsc::Receiver<(Message, oneshot::Sender<Message>)>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let model = TextEmbedding::try_new(Default::default())?;
    info!("Loaded model");

    let mut config = Arc::new(RwLock::new(get_config().await?));
    info!("Loaded Config");

    info!("Starting maintainer");
    while let Some((message, response)) = rx.recv().await {
        match message {
            Message::Search(_) => todo!(),
            Message::Get(_) => todo!(),
            Message::Track(path, args) => {
                let config = config.clone();
                let paths = tokio_rayon::spawn(|| crawl_directory(path, args, track))
                    .await
                    .unwrap_or_default();

                // Track all paths
                config.write().await.tracked.extend(paths);
            }
            Message::Untrack(_) => todo!(),
            Message::Paths(_) => todo!(),
            Message::Confirmation => todo!(),
        };
    }
    Ok(())
}

async fn socket_listener(
    tx: mpsc::Sender<(Message, oneshot::Sender<Message>)>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if std::path::Path::exists(Path::new("/tmp/vs.sock")) {
        std::fs::remove_file("/tmp/vs.sock")
            .with_context(|| "Unable to delete existing unix socket")?;
    }
    let listener =
        UnixListener::bind("/tmp/vs.sock").with_context(|| "Unable to bind unix socket")?;

    info!("Starting socket listener");
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let tx = tx.clone();
                tokio::task::spawn(async move { handle_connection(stream, tx, addr).await });
            }
            Err(e) => {
                error!(error = %e, "error accepting socket");
            }
        };
    }
}

#[tokio::main]
async fn main() {
    initialize_db().await.unwrap();
    tracing_subscriber::fmt::init();
    let (tx, rx) = mpsc::channel::<(Message, oneshot::Sender<Message>)>(1);

    let mut t1 = tokio::task::spawn(socket_listener(tx));
    let mut t2 = tokio::task::spawn(maintainer(rx));

    let e = select!(
        r = &mut t1 => {t2.abort(); r.err()},
        r = &mut t2 => {t1.abort(); r.err()}
    );

    error!(reason = ?e, "critical task closed, shutting down service");

    // let results = crawl_directory(
    //     PathBuf::from_str(&args().collect::<Vec<_>>()[1]).unwrap(),
    //     |p| {
    //         println!("{}", p.display());
    //         p
    //     },
    // )
    // .unwrap()
    // .iter()
    // .map(|p| p.display().to_string())
    // .collect();

    // println!("{:?}", results);
    // println!("{:?}", model.embed(results, None).unwrap()[0]);
}

async fn initialize_db() -> Result<Connection, Box<dyn Error>> {
    let db = lancedb::connect("/tmp/vs-db").execute().await?;

    let schema = Arc::new(Schema::new(vec![
        Field::new("path", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), 128),
            true,
        ),
    ]));

    if !db
        .table_names()
        .execute()
        .await?
        .contains(&"index".to_string())
    {
        let tbl = db
            .create_empty_table("index", schema.clone())
            .execute()
            .await?;
    }

    Ok(db)
    // // Create a RecordBatch stream.
    // let batches = RecordBatchIterator::new(
    //     vec![RecordBatch::try_new(
    //         schema.clone(),
    //         vec![
    //             Arc::new(Int32Array::from_iter_values(0..256)),
    //             Arc::new(
    //                 FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
    //                     (0..256).map(|_| Some(vec![Some(1.0); 128])),
    //                     128,
    //                 ),
    //             ),
    //         ],
    //     )
    //     .unwrap()]
    //     .into_iter()
    //     .map(Ok),
    //     schema.clone(),
    // );

    // let tbl = match db.open_table("index").execute().await {
    //     Ok(tbl) => tbl,
    //     Err(e) => db
    //         .create_table("index", Box::new(batches))
    //         .execute()
    //         .await
    //         .unwrap(),
    // };

    // let mut options = QueryExecutionOptions::default();
    // options.max_batch_length = 1;
    // let results = tbl
    //     .query()
    //     .nearest_to(&[1.0; 128])
    //     .unwrap()
    //     .limit(1)
    //     .execute()
    //     .await
    //     .unwrap()
    //     .try_collect::<Vec<_>>()
    //     .await
    //     .unwrap();

    // println!("{:?}", results);
}