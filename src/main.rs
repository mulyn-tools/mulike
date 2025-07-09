use anyhow::Result;
use axum::{
    Router,
    extract::{Query, State},
    response::{IntoResponse, Response},
    routing::get,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use tracing::error;
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Deserialize)]
struct Captain {
    // code: i32,
    data: CaptainData,
}

#[derive(Debug, Deserialize)]
struct CaptainData {
    info: CaptainDataInfo,
    list: Vec<CaptainEntry>,
    top3: Option<Vec<CaptainEntry>>,
}

#[derive(Debug, Deserialize)]
struct CaptainDataInfo {
    page: i32,
}

#[derive(Debug, Deserialize)]
struct CaptainEntry {
    username: String,
}

// learned from https://github.com/tokio-rs/axum/blob/main/examples/anyhow-error-response/src/main.rs
pub struct AnyhowError(anyhow::Error);

impl IntoResponse for AnyhowError {
    fn into_response(self) -> Response {
        error!("Returning internal server error for {}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", self.0)).into_response()
    }
}

impl<E> From<E> for AnyhowError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

#[derive(Debug, Clone, Copy)]
struct ShareState {
    roomid: u32,
    ruid: u32,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let local_url = std::env::var("LOCAL_URL").expect("LOCAL_URL is not set");

    let roomid = std::env::var("ROOMID")
        .expect("ROOMID is not set")
        .parse::<u32>()
        .expect("Failed to parse roomid");

    let ruid = std::env::var("RUID")
        .expect("RUID is not set")
        .parse::<u32>()
        .expect("Failed to parse ruid");

    // initialize tracing
    let env_log = EnvFilter::try_from_default_env();

    if let Ok(filter) = env_log {
        tracing_subscriber::registry()
            .with(fmt::layer().with_filter(filter))
            .init();
    } else {
        tracing_subscriber::registry().with(fmt::layer()).init();
    }

    let app = Router::new()
        .route("/", get(get_list))
        .with_state(ShareState { roomid, ruid });

    let listener = tokio::net::TcpListener::bind(local_url).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[derive(Debug, Deserialize)]
struct QueryUsername {
    username: Option<String>,
}

async fn get_list(
    State(ShareState { roomid, ruid }): State<ShareState>,
    Query(QueryUsername { username }): Query<QueryUsername>,
) -> Result<impl IntoResponse, AnyhowError> {
    let list = get_list_inner(roomid, ruid).await?;

    if let Some(username) = username {
        let res = list
            .into_iter()
            .filter(|u| u.contains(&username))
            .collect::<Vec<_>>();

        return Ok(res.join("\n"));
    }

    Ok(list.join("\n"))
}

async fn get_list_inner(roomid: u32, ruid: u32) -> Result<Vec<String>> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0")
        .build()
        .unwrap();

    let mut page = 1;

    let mut res = vec![];

    loop {
        let resp = client
            .get("https://api.live.bilibili.com/xlive/app-room/v2/guardTab/topList")
            .query(&[
                ("roomid", roomid.to_string()),
                ("ruid", ruid.to_string()),
                ("page", page.to_string()),
                ("page_size", "30".to_string()),
            ])
            .send()
            .await?
            .error_for_status()?;

        let c = resp.json::<Captain>().await?;

        if c.data.info.page < page {
            return Ok(res);
        }

        if page == 1 {
            if let Some(top3) = c.data.top3 {
                for i in top3 {
                    res.push(i.username);
                }
            }
        }

        let list = c.data.list;

        for i in list {
            res.push(i.username);
        }

        page += 1;
    }
}
