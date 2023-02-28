use std::sync::Arc;

use warp::{Filter, Reply};
use serde::{Deserialize, Serialize};

use sqlx::{migrate::MigrateDatabase, Sqlite, SqlitePool, query, query_as, FromRow, /*Row, sqlite::SqliteRow*/};

use tracing::{Level, info, error};
use tracing_subscriber::FmtSubscriber;

static DB_URL: &str = "sqlite://pod.sql";

#[tokio::main]
async fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber).unwrap();

    match Sqlite::create_database(DB_URL).await {
        Ok(()) => {
            info!("Created database {}", DB_URL);
        }
        Err(e) => {
            let sqlx::Error::Database(db_err) = e else {
                panic!("error creating database: {e}");
            };

            panic!("sql db error: {db_err:?}");//.code()
        }
    }

    let db = SqlitePool::connect(DB_URL)
        .await
        .expect("DB connection");

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migration");

    let db = Arc::new(db);

    let login = warp::path!("api" / "2" / "auth" / String / "login.json")
        .and(warp::post())
        .and(warp::header("authorization"))
        .map(|username, auth: String| {
            eprintln!("todo: auth or {username}: {auth}");

            warp::reply::with_status(
                warp::reply(),
                warp::http::StatusCode::OK // UNAUTHORIZED
            )
        });

    let devices = {
        let for_user = warp::path!("api" / "2" / "devices" / String)
            .and(warp::get())
            .then({
                let db = Arc::clone(&db);
                move |username_format: String| {
                    let db = Arc::clone(&db);

                    async move {
                        let (username, format) = match username_format.split_once('.') {
                            Some(tup) => tup,
                            None => return warp::reply::with_status(
                                warp::reply(),
                                warp::http::StatusCode::BAD_REQUEST
                                ).into_response(),
                        };

                        if format != "json" {
                            return warp::reply::with_status(
                                warp::reply(),
                                warp::http::StatusCode::UNPROCESSABLE_ENTITY,
                                ).into_response();
                        }

                        let query = query_as!(
                            Device,
                            r#"
                            SELECT id, caption, type as "type: _", subscriptions, username
                            FROM devices
                            WHERE username = ?
                            "#,
                            username,
                        )
                            .fetch_all(&*db)
                            .await;

                        let devices = match query {
                            Ok(d) => d,
                            Err(e) => {
                                error!("select error: {:?}", e);

                                return warp::reply::with_status(
                                    warp::reply(),
                                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                                ).into_response();
                            }
                        };

                        // let devices: [Device; 1] = [
                        //     Device {
                        //         id: "test".into(),
                        //         caption: "test".into(),
                        //         r#type: DeviceType::Mobile,
                        //         subscriptions: 1,
                        //     },
                        // ];

                        warp::reply::json(&devices).into_response()
                    }
                }
            });

        let create = warp::path!("api" / "2" / "devices" / String / String)
            .and(warp::post())
            .and(warp::body::json()) // TODO: this may just be an empty string
            .then({
                let db = Arc::clone(&db);
                move |username, device_name, new_device: DeviceCreate| {
                    let db = Arc::clone(&db);
                    async move {
                        // device_name is device id
                        // FIXME: use device_name
                        println!("got device creation {device_name} for {username}: {new_device:?}");

                        let caption = new_device.caption.as_deref().unwrap_or("");
                        let r#type = new_device.r#type.unwrap_or(DeviceType::Unknown);

                        let query = query!(
                            "INSERT INTO devices
                            (caption, type, username, subscriptions)
                            VALUES
                            (?, ?, ?, ?)",
                            caption,
                            r#type,
                            username,
                            0,
                        )
                            .execute(&*db)
                            .await;

                        match query {
                            Ok(_) => warp::reply().into_response(),
                            Err(e) => {
                                // FIXME: handle EEXIST (and others?)
                                error!("insert error: {:?}", e);

                                warp::reply::with_status(
                                    warp::reply(),
                                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                                    ).into_response()
                            }
                        }
                    }
                }
            });

        for_user.or(create)
    };

    let subscriptions = {
        let get = warp::path!("api" / "2" / "subscriptions" / String / String) // FIXME: merge this
                                                                               // with the below path (same for /episodes)
            .and(warp::get())
            .map(|username, deviceid_format| {
                println!("got subscriptions for {deviceid_format} for {username}");

                warp::reply::json(&SubscriptionChanges {
                    add: vec![],
                    remove: vec![],
                    timestamp: Some(0),
                })
            });

        let upload = warp::path!("api" / "2" / "subscriptions" / String / String)
            .and(warp::post())
            .and(warp::body::json())
            .then({
                let db = Arc::clone(&db);
                move |username, deviceid_format, sub_changes: SubscriptionChanges| {
                    let db = Arc::clone(&db);

                    async move {
                        println!("got urls for {username}'s device {deviceid_format}, timestamp {:?}:", sub_changes.timestamp);

                        let mut tx = match db.begin().await {
                            Ok(tx) => tx,
                            Err(e) => {
                                error!("transaction begin: {:?}", e);

                                return warp::reply::with_status(
                                    warp::reply(),
                                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                                    ).into_response()
                            }
                        };

                        for url in &sub_changes.remove {
                            let query = query!(
                                "DELETE FROM subscriptions WHERE url = ?",
                                url
                            )
                                .execute(&mut tx)
                                .await;

                            if let Err(e) = query {
                                error!("transaction addition: {:?}", e);

                                return warp::reply::with_status(
                                    warp::reply(),
                                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                                ).into_response();
                            }
                        }

                        for url in &sub_changes.add {
                            let query = query!(
                                "
                                INSERT INTO subscriptions
                                (url, username, device)
                                VALUES
                                (?, ?, ?)
                                ",
                                url,
                                username,
                                device,
                            )
                                .execute(&mut tx)
                                .await;

                            if let Err(e) = query {
                                error!("transaction addition: {:?}", e);

                                return warp::reply::with_status(
                                    warp::reply(),
                                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                                ).into_response();
                            }
                        }

                        match tx.commit().await {
                            Ok(()) => {
                                warp::reply::json(
                                    &UpdatedUrls {
                                        timestamp: 0, // TODO
                                        update_urls: vec![], // unused by client
                                    })
                                .into_response()
                            }
                            Err(e) => {
                                error!("transaction commit: {:?}", e);

                                warp::reply::with_status(
                                    warp::reply(),
                                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                                    ).into_response()
                            }
                        }
                    }
                }
            });

        get.or(upload)
    };

    let episodes = {
        let get = warp::path!("api" / "2" / "episodes" / String)
            .and(warp::get())
            .and(warp::query())
            .map(|username_format, query: QuerySince| {
                println!("episodes GET for {username_format} since {query:?}");

                warp::reply::json(
                    &EpisodeChanges {
                        timestamp: 0,
                        actions: vec![],
                    })
            });

        let upload = warp::path!("api" / "2" / "episodes" / String)
            .and(warp::post())
            .and(warp::body::json())
            .map(|username_format, body: Vec<EpisodeActionUpload>| {
                println!("episodes POST for {username_format}");

                for action in &body {
                    println!("  {:?}", action);
                }

                warp::reply::json(
                    &UpdatedUrls { // FIXME: rename struct
                        timestamp: 0, // FIXME: timestamping
                        update_urls: vec![], // unused by client
                    })
            });

        get.or(upload)
    };

    let routes = login
        .or(devices)
        .or(subscriptions)
        .or(episodes)
        .with(warp::trace::request());

    warp::serve(routes)
        .run(([0, 0, 0, 0], 8080))
        .await;
}

#[derive(Debug, Serialize, FromRow)]
struct Device {
    id: i64, // FIXME: String, convert when pulling out of the DB? change the DB type?
    caption: String,

    // #[sqlx(try_from = "String")]
    r#type: DeviceType,

    subscriptions: i64,

    #[serde(skip)]
    username: String,
}

// impl FromRow<'_, SqliteRow> for Device {
//     fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
//         let ty: &str = row.try_get("type")?;
//         Ok(Self {
//             id: row.try_get("id")?,
//             caption: row.try_get("caption")?,
//             r#type: ty.try_into().unwrap(),
//             subscriptions: row.try_get("subscriptions")?,
//             username: row.try_get("username")?,
//         })
//     }
// }

#[derive(Debug, Deserialize, Serialize)] // FIXME: drop Serialize
struct DeviceCreate { // FIXME: allow "" to deserialise to this
    caption: Option<String>,
    r#type: Option<DeviceType>,
}

#[derive(Debug, Deserialize, Serialize, sqlx::Type)]
// #[sqlx(transparent)]
#[serde(rename_all = "lowercase")]
enum DeviceType {
    Mobile,
    Unknown,
}

impl TryFrom<&'_ str> for DeviceType {
    type Error = ();

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "Mobile" => Ok(DeviceType::Mobile),
            _ => Err(())
        }
    }
}

impl TryFrom<String> for DeviceType {
    type Error = ();

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from(&*s)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct DeviceId(String);

#[derive(Debug, Deserialize, Serialize)]
struct Subscription {
    url: String,
    title: String,
    author: String,
    description: String,
    subscribers: u32,
    logo_url: String,
    scaled_logo_url: String,
    website: String,
    mygpo_link: String,
}
// let subscriptions: [&'static str; 1] = [
//     "http://test.com",
//     // Subscription {
//     //     url: "http://test.com".into(),
//     //     title: "test pod".into(),
//     //     author: "rob".into(),
//     //     description: "a test podcast".into(),
//     //     subscribers: 2,
//     //     logo_url: "https://avatars.githubusercontent.com/u/205673?s=40&v=4".into(),
//     //     scaled_logo_url: "https://avatars.githubusercontent.com/u/205673?s=40&v=4".into(),
//     //     website: "https://github.com/bobrippling".into(),
//     //     mygpo_link: "https://github.com/bobrippling".into(),
//     // },
// ];

#[derive(Debug, Deserialize, Serialize)]
struct SubscriptionChanges {
    add: Vec<String>, // TODO: make these &str?
    remove: Vec<String>,
    timestamp: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct EpisodeChanges {
    timestamp: u32,
    actions: Vec<EpisodeAction>,
}

#[derive(Debug, Deserialize, Serialize)]
struct QuerySince {
    since: u32,
}

#[derive(Debug, Deserialize, Serialize)]
struct EpisodeAction {
    podcast: String,
    episode: String,
    #[serde(with = "time::serde::rfc3339")]
    timestamp: time::OffsetDateTime, // yyyy-MM-dd'T'HH:mm:ss;
    guid: Option<String>,
    action: EpisodeActionE,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase", untagged)]
enum EpisodeActionE {
    New,
    Download,
    Play {
        started: u32,
        position: u32,
        total: u32,
    },
    Delete,
}

#[derive(Debug, Deserialize, Serialize)]
struct EpisodeActionUpload {
    podcast: String,
    episode: String,
    #[serde(with = "time_custom")]
    timestamp: time::PrimitiveDateTime,
    guid: Option<String>,
    #[serde(flatten)] // TODO: use this to combine common fields across types
    action: EpisodeActionE, // FIXME: spread EpisodeActionE into this type
    device: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct UpdatedUrls {
    timestamp: u32,
    update_urls: Vec<[String; 2]>,
}

time::serde::format_description!( // FIXME: swap to chrono & Utc.datetime_from_str(<time>, <fmt>) ?
    time_custom,
    PrimitiveDateTime,
    "[year]-[month]-[day]T[hour]:[minute]:[second]" // yyyy-MM-dd'T'HH:mm:ss
);
