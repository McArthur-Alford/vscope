use anyhow::anyhow;
use anyhow::{Context, Result};
use arrow::array::{
    Array, ArrayData, DictionaryArray, FixedSizeListArray, Float32Array, Int32Array, RecordBatch,
    RecordBatchIterator, StringArray,
};
use arrow::datatypes::{DataType, Field, Float32Type, Schema, ToByteSlice, Utf8Type};
use arrow::ipc::{FixedSizeList, Utf8};
use axum::extract::FromRef;
use fastembed::{TextEmbedding, TextRerank};
use futures_util::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, QueryExecutionOptions};
use lancedb::{Connection, Table};
use rayon::iter::{ParallelBridge, ParallelExtend, ParallelIterator};
use serde::{Deserialize, Serialize};
use std::char;
use std::collections::{HashSet, VecDeque};
use std::env::args;
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::str::Chars;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::{fs::read_dir, io, path::PathBuf, str::FromStr};
use tokio::io::{AsyncReadExt, AsyncWriteExt, Interest};
use tokio::net::unix::SocketAddr;
use tokio::net::{UnixListener, UnixStream};
use tokio::select;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, RwLock};
use tracing::{error, info, instrument, warn};
use vs_core::{Message, TrackArgs};

#[derive(Serialize, Deserialize, Default)]
struct Config {
    tracked: HashSet<PathBuf>,
}

impl Config {
    fn save(&self) {
        info!("Beginning config save");
        let message = match serde_json::to_string(self) {
            Ok(message) => message,
            Err(e) => {
                error!(error=?e, "Unable to serialize config");
                return;
            }
        };

        let path = Path::new("/tmp/vs.config");
        let mut opts = OpenOptions::new();
        opts.write(true).truncate(true).create(true);
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
        info!("Finished config save");
    }
}

fn crawl_directory<F: Send + Sync + Copy + Fn(&PathBuf, &TrackArgs, &mut mpsc::Sender<PathBuf>)>(
    dir: PathBuf,
    args: TrackArgs,
    mut tx: mpsc::Sender<PathBuf>,
    f: F,
) -> io::Result<Vec<PathBuf>> {
    // info!(dir=%dir.display(), "Crawled");
    f(&dir, &args, &mut tx);
    let mut out = vec![dir.clone()];
    if dir.is_dir() && args.recursive && (!dir.is_symlink() || args.follow_symlinks) {
        out.extend({
            let entries = read_dir(dir)?;
            let results = entries
                .par_bridge()
                .filter_map(Result::ok)
                .filter_map(|p| crawl_directory(p.path(), args.clone(), tx.clone(), f).ok())
                .flatten()
                .collect::<Vec<PathBuf>>();
            results
        });
    }
    Ok(out)
}

#[instrument(skip_all)]
async fn handle_connection(
    mut stream: UnixStream,
    tx: mpsc::Sender<(Message, oneshot::Sender<Message>)>, // Send messages to daemon
    addr: SocketAddr,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!(addr=?addr, "Established Connection");
    let mut outbound = VecDeque::<Message>::new();
    loop {
        let ready = stream
            .ready(Interest::READABLE | Interest::WRITABLE)
            .await?;

        while ready.is_writable() && !outbound.is_empty() {
            info!("Writing");
            let message = serde_json::to_vec(&outbound.pop_back())
                .with_context(|| "Failed to serialize message")?;
            info!(len=%message.len(), "Response length");
            stream.write_u32(message.len() as u32).await?;
            stream.flush().await?;
            stream
                .write_all(&message)
                .await
                .with_context(|| "Unable to write to socket")?;
            stream
                .flush()
                .await
                .with_context(|| "Unable to flush socket")?;
            info!("Finished writing");
        }

        if ready.is_readable() {
            info!("Reading");

            let n = stream.read_u32().await?;
            info!(len=?n, "Got message length");
            let mut message = vec![0; n as usize];
            let _ = stream.read_exact(&mut message).await?;

            match serde_json::from_slice(&message)
                .with_context(|| "Failed to parse message over socket")
            {
                Ok(message) => {
                    let (otx, orx) = oneshot::channel();
                    info!(message=?message, "Received message");
                    tx.send((message, otx)).await?;
                    let out = orx.await.with_context(|| "Failed to read callback")?;
                    outbound.push_front(out);
                }
                Err(e) => error!(err=?e, "Failed to decode message"),
            }

            info!("Finished reading")
        }
    }
}

#[instrument(skip_all)]
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

fn track(path: &PathBuf, args: &TrackArgs, tx: &mut mpsc::Sender<PathBuf>) {
    // Embed the path and save it to the database
    if !path.exists() {
        return;
    }

    tx.blocking_send(path.clone());
    // if path.is_dir() && !args.recursive {
    // }
    // if path.is_file() && true {
    //     // TODO regex, etc
    //     tx.blocking_send(path.clone());
    // }
}

#[instrument(skip_all)]
async fn maintainer(
    mut rx: mpsc::Receiver<(Message, oneshot::Sender<Message>)>,
) -> anyhow::Result<()> {
    let model = TextEmbedding::try_new(Default::default())?;
    info!("Loaded model");
    let mut db = initialize_db().await.map_err(|e| anyhow!("Failed"))?;

    let mut config = Arc::new(RwLock::new(
        get_config()
            .await
            .map_err(|e| anyhow!("Failed to get config"))?,
    ));
    info!("Loaded Config");

    let (encoder_tx, mut encoder_rx) = mpsc::channel(1000);
    tokio::task::spawn(encoder(encoder_rx, model));
    tokio::task::spawn(db_indexer());

    let mut got = Vec::new();
    let model = TextEmbedding::try_new(Default::default())?;
    info!("Starting maintainer");
    while let Some((message, response)) = rx.recv().await {
        info!(message=?message, "Maintainer Received message");
        match message {
            Message::Search(path) => {
                info!("Handling search");
                let query = model.embed(vec![path], None)?.get(0).unwrap().clone();
                got = db_get(&mut db, query).await.unwrap_or_default();
                info!(?got);
                response
                    .send(Message::Confirmation)
                    .map_err(|e| anyhow!("Failed to send response"))?;
                info!("Finished search");
            }
            Message::Get(n) => {
                let paths = if got.len() > 0 {
                    got.iter().cloned().take(n).collect()
                } else {
                    Vec::new()
                };
                response
                    .send(Message::Paths(paths))
                    .map_err(|e| anyhow!("Failed to send response"))?;
            }
            Message::Track(path, args) => {
                info!("Handling track");
                response
                    .send(Message::Confirmation)
                    .map_err(|e| anyhow!("Failed to send response"))?;
                let args = args.clone();
                let config = config.clone();
                let encoder_tx = encoder_tx.clone();
                let path2 = path.clone();
                tokio::task::spawn(async move {
                    let Ok(mut db) = initialize_db().await.map_err(|e| anyhow!("Failed")) else {
                        return;
                    };
                    let paths =
                        tokio_rayon::spawn(move || crawl_directory(path, args, encoder_tx, track))
                            .await
                            .unwrap_or_default();

                    info!(path=?path2, "Finished crawling for track");

                    // Track all paths
                    {
                        config.write().await.tracked.extend(paths);
                    }
                    config.read().await.save();

                    db_index(&mut db).await;
                });
            }
            Message::Untrack(path, args) => {
                info!("Handling untrack");
                response
                    .send(Message::Confirmation)
                    .map_err(|e| anyhow!("Failed to send response"))?;
                let encoder_tx = encoder_tx.clone();
                let config = config.clone();
                let paths = tokio_rayon::spawn(move || {
                    crawl_directory(path, args, encoder_tx.clone(), |_, _, _| {})
                })
                .await
                .unwrap_or_default();
                info!("Finished crawling for untrack");

                println!("{:?}", paths);
                // Untrack the paths
                {
                    let tracked = &mut config.write().await.tracked;
                    for item in &paths {
                        tracked.remove(item);
                    }
                    db_remove(&mut db, paths).await?;
                }

                config.read().await.save();
                info!("Finished untrack");
            }
            Message::Paths(_) => panic!(),
            Message::Confirmation => panic!(),
        };
    }
    Ok(())
}

#[instrument(skip_all)]
async fn encoder(mut encoder_rx: mpsc::Receiver<PathBuf>, model: TextEmbedding) {
    let model = Arc::new(Mutex::new(model));
    let mut db = Arc::new(Mutex::new(initialize_db().await.expect("Expected DB")));
    let mut zc = 0;
    loop {
        let mut buffer = Vec::<PathBuf>::new();
        let amount = encoder_rx.recv_many(&mut buffer, 100).await;
        if amount == 0 {
            zc += 1;
            if zc % 100 == 0 {
                warn!(times=?zc, "Tried to encode nothing several times");
            }
            continue;
        }
        zc = 0;
        info!(num_files=?amount, "Getting contents");
        info!(buffer=?buffer);

        let names = buffer
            .iter()
            .filter_map(|path| {
                let name = path.file_name().map(|s| s.to_string_lossy().to_string());
                println!("{:?}", name);
                if path.is_file() && binaryornot::is_binary(path).unwrap_or(false) {
                    warn!(bin=?path, "Skipping binary");
                    name
                } else if path.is_file() {
                    if let Ok(file) = File::open(path) {
                        let mut reader = BufReader::new(file);
                        let mut buf = Vec::new();
                        if let Err(e) = reader.take(100000).read_to_end(&mut buf) {
                            error!("{:?}", e);
                            return name;
                        }
                        let buf = buf
                            .iter()
                            .filter_map(|c| char::from_u32(*c as u32))
                            .collect();
                        Some([name.unwrap_or("".to_string()), buf].join(": "))
                    } else {
                        name
                    }
                } else if path.is_dir() {
                    name.map(|name| format!("{}: ", name))
                } else {
                    name
                }
            })
            .collect();
        info!("Starting encode");
        info!(?names);

        let model = model.clone();
        let db = db.clone();
        let t = tokio::task::spawn(async move {
            let Ok(embeddings) = model.lock().await.embed(names, Some(amount)) else {
                warn!("Encoding failed");
                return;
            };
            info!("Finished Encoding");

            insert_db(db, embeddings, buffer).await;
        });

        let timer = tokio::time::timeout(tokio::time::Duration::from_secs(5), t).await;
    }
}

#[instrument(skip_all)]
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
#[instrument(skip_all)]
async fn main() {
    tracing_subscriber::fmt::init();
    let (tx, rx) = mpsc::channel::<(Message, oneshot::Sender<Message>)>(1);

    let mut t1 = tokio::task::spawn(socket_listener(tx));
    let mut t2 = tokio::task::spawn(maintainer(rx));

    let e = select!(
        r = &mut t1 => {t2.abort(); r.err()},
        r = &mut t2 => {t1.abort(); r.err()}
    );

    error!(reason = ?e, "critical task closed, shutting down service");
}

#[instrument(skip_all)]
async fn insert_db(db: Arc<Mutex<Connection>>, encodings: Vec<Vec<f32>>, paths: Vec<PathBuf>) {
    let db = db.lock().await;
    info!("Inserting into DB");
    if encodings.len() == 0 {
        info!("Zero encodings, nothing to insert");
        return;
    }
    let length = encodings[0].len();
    let amount = encodings.len();
    let schema = db_schema().await;
    // Create a RecordBatch stream.
    let batches = RecordBatchIterator::new(
        vec![RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(
                    paths
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect::<Vec<_>>(),
                )),
                Arc::new(
                    FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                        encodings.iter().map(|v| Some(v.iter().map(|f| Some(*f)))),
                        length as i32,
                    ),
                ),
            ],
        )
        .unwrap()]
        .into_iter()
        .map(Ok),
        schema.clone(),
    );

    let tbl = match db.open_table("index").execute().await {
        Ok(tbl) => tbl,
        Err(e) => db
            .create_empty_table("index", schema)
            .execute()
            .await
            .unwrap(),
    };

    let mut merge_insert = tbl.merge_insert(&["path"]);
    merge_insert
        .when_matched_update_all(None)
        .when_not_matched_insert_all();
    if let Err(e) = merge_insert.execute(Box::new(batches)).await {
        error!(err=?e, "Unable to add batches");
    }

    info!("Done inserting into db");
}

#[instrument(skip_all)]
async fn db_indexer() -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(100));
    loop {
        interval.tick().await;
        let mut db = initialize_db()
            .await
            .map_err(|e| anyhow!("Indexer failed to connect to db"))?;
        info!("Beginning automatic index");
        if let Err(e) = db_index(&mut db).await {
            error!(error=?e, "Failed to index db");
        }
        info!("Finished automatic index");
    }
    Ok(())
}

#[instrument(skip_all)]
async fn db_index(db: &mut Connection) -> anyhow::Result<()> {
    let tbl = db_get_table(db).await?;
    let rows = tbl.count_rows(None).await?;
    if rows < 256 {
        warn!(rows=?rows, "Not enough rows yet to index");
        return Ok(());
    }
    if let Err(e) = tbl
        .create_index(&["vector"], lancedb::index::Index::Auto)
        .execute()
        .await
    {
        error!(error=?e, "Failed to create index");
        return Err(anyhow!("Failed to create index"));
    }
    info!("Index updated");
    Ok(())
}

#[instrument(skip_all)]
async fn db_get_table(db: &mut Connection) -> anyhow::Result<Table> {
    let schema = db_schema().await;
    let tbl = match db.open_table("index").execute().await {
        Ok(tbl) => tbl,
        Err(e) => db.create_empty_table("index", schema).execute().await?,
    };
    Ok(tbl)
}

#[instrument(skip_all)]
async fn db_get(db: &mut Connection, query: Vec<f32>) -> anyhow::Result<Vec<PathBuf>> {
    info!("DB_GET");
    let tbl = db_get_table(db).await?;

    let mut options = QueryExecutionOptions::default();
    options.max_batch_length = 1;
    let results = tbl
        .query()
        .nearest_to(query.as_slice())
        .unwrap()
        // .limit(amount)
        .execute()
        .await
        .unwrap()
        .try_collect::<Vec<_>>()
        .await
        .unwrap();

    let b = results.get(0).context("is the db empty? failed to read")?;
    let cols = b.columns();
    let names = cols[0].as_any().downcast_ref::<StringArray>().unwrap();
    let ranks = cols[2].as_any().downcast_ref::<Float32Array>().unwrap();

    let ranks = ranks.iter().filter_map(|t| t).collect::<Vec<_>>();

    let names = names
        .iter()
        .filter_map(|t| t)
        .filter_map(|t| PathBuf::from_str(t).ok())
        .collect::<Vec<_>>();

    let mut zipped = ranks.iter().zip(names).collect::<Vec<_>>();
    zipped.sort_by(|a, b| a.0.partial_cmp(b.0).unwrap());

    Ok(zipped.into_iter().map(|(_, b)| b).collect())
}

#[instrument(skip_all)]
async fn db_remove(db: &mut Connection, paths: Vec<PathBuf>) -> anyhow::Result<()> {
    let tbl = db_get_table(db).await?;
    let list = format!(
        "({})",
        paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .map(|p| format!("\"{}\"", p))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let query = &format!("path in {}", list);
    println!("{}", query);
    if let Err(e) = tbl.delete(query).await {
        error!(error=?e, "Failed to delete rows");
    }
    Ok(())
}

#[instrument(skip_all)]
async fn db_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("path", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), 384),
            true,
        ),
    ]))
}

#[instrument(skip_all)]
async fn initialize_db() -> Result<Connection, Box<dyn Error>> {
    let db = lancedb::connect("/tmp/vs-db").execute().await?;
    let schema = db_schema().await;

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
}
