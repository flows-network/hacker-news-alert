use anyhow;
use chrono::{Duration, Utc};
use dotenv::dotenv;
use http_req::{request, request::Method, request::Request, uri::Uri};
use openai_flows::{
    chat::{ChatModel, ChatOptions},
    OpenAIFlows,
};
use schedule_flows::schedule_cron_job;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use slack_flows::{listen_to_channel, send_message_to_channel};
use std::env;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
// use web_scraper_flows::get_page_text;

#[no_mangle]
pub fn run() {
    dotenv().ok();
    let keyword = std::env::var("KEYWORD").unwrap_or("chatGPT".to_string());
    schedule_cron_job(String::from("56 * * * *"), keyword, callback);
}

#[no_mangle]
#[tokio::main(flavor = "current_thread")]
async fn callback(keyword: Vec<u8>) {
    let workspace = env::var("slack_workspace").unwrap_or("secondstate".to_string());
    let channel = env::var("slack_channel").unwrap_or("github-status".to_string());

    let query = String::from_utf8_lossy(&keyword);
    let now = SystemTime::now();
    let dura = now.duration_since(UNIX_EPOCH).unwrap().as_secs() - 3600;
    let url = format!("https://hn.algolia.com/api/v1/search_by_date?tags=story&query={query}&numericFilters=created_at_i>{dura}");

    let mut writer = Vec::new();
    if let Ok(_) = request::get(url, &mut writer) {
        if let Ok(search) = serde_json::from_slice::<Search>(&writer) {
            for hit in search.hits {
                let title = &hit.title;
                let url = &hit.url;
                let object_id = &hit.object_id;
                let author = &hit.author;

                let post = format!("https://news.ycombinator.com/item?id={object_id}");

                match url {
                    Some(u) => {
                        let source = format!("(<{u}|source>)");
                        if let Some(text) = obtain_text_by_post(u).await {
                            match get_summary_truncated(&text).await {
                                Ok(summary) => {
                                    let msg = format!(
                                        "- *{title}*\n<{post} | post>{source} by {author}\n{text}"
                                    );
                                    send_message_to_channel(&workspace, &channel, msg).await;
                                }
                                Err(_e) => {
                                    send_message_to_channel("ik8", "ch_err", _e.to_string()).await
                                }
                            }
                        }
                    }
                    None => {
                        if let Some(text) = obtain_text_by_post(&post).await {
                            if let Ok(summary) = get_summary_truncated(&text).await {
                                let msg =
                                    format!("- *{title}*\n<{post} | post> by {author}\n{summary}");
                                send_message_to_channel(&workspace, &channel, msg).await;
                            }
                        }
                    }
                };
            }
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Search {
    pub hits: Vec<Hit>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Hit {
    pub title: String,
    pub url: Option<String>,
    #[serde(rename = "objectID")]
    pub object_id: String,
    pub author: String,
    pub created_at_i: i64,
}

async fn get_summary_truncated(inp: &str) -> anyhow::Result<String> {
    let mut openai = OpenAIFlows::new();
    openai.set_retry_times(3);

    let news_body = inp
        .split_ascii_whitespace()
        .take(12000)
        .collect::<Vec<&str>>()
        .join(" ");

    let chat_id = format!("news summary N");
    let system = &format!("You're an editor AI.");

    let co = ChatOptions {
        model: ChatModel::GPT35Turbo16K,
        restart: true,
        system_prompt: Some(system),
    };

    let question = format!("Summarize this: {news_body}.");

    match openai.chat_completion(&chat_id, &question, &co).await {
        Ok(r) => Ok(r.choice),
        Err(_e) => {
            send_message_to_channel("ik8", "ch_err", _e.to_string()).await;
            Err(anyhow::Error::msg(_e.to_string()))
        }
    }
}

pub async fn obtain_text_by_post(inp: &str) -> Option<String> {
    let server_addr = "43.135.155.64:3000".parse::<SocketAddr>().unwrap();
    let url = format!("http://{}/api", server_addr);
    let uri = Uri::try_from(url.as_ref()).unwrap();
    let body = json!({ "url": inp }).to_string();

    let mut writer = Vec::<u8>::new();
    if let Ok(_res) = Request::new(&uri)
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .header("Content-Length", &body.len())
        .body(body.as_bytes())
        .send(&mut writer)
    {
        let text_load = String::from_utf8_lossy(&writer);
        return Some(text_load.to_string());
    }
    None
}

pub async fn obtain_summary_by_post(inp: &str) -> Option<String> {
    let server_addr = "43.135.155.64:4000".parse::<SocketAddr>().unwrap();
    let url = format!("http://{}/api", server_addr);
    let uri = Uri::try_from(url.as_ref()).unwrap();
    let body = json!({ "url": inp }).to_string();

    let mut writer = Vec::<u8>::new();
    if let Ok(_res) = Request::new(&uri)
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .header("Content-Length", &body.len())
        .body(body.as_bytes())
        .send(&mut writer)
    {
        let text_load = String::from_utf8_lossy(&writer);
        return Some(text_load.to_string());
    }
    None
}
