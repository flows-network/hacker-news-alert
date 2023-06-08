use std::time::{SystemTime, UNIX_EPOCH};
use dotenv::dotenv;
use http_req::request;
use openai_flows::{chat, Model, OpenAIFlows};
use schedule_flows::schedule_cron_job;
use serde_derive::{Deserialize, Serialize};
use slack_flows::send_message_to_channel;
use std::env;

#[no_mangle]
pub fn run() {
    schedule_cron_job(
        String::from("50 * * * *"),
        "chron job scheduled".to_string(),
        callback,
    );
}

fn callback(keyword: Vec<u8>) {
    dotenv().ok();

    let keyword = std::env::var("KEYWORD").unwrap();
    let workspace = std::env::var("slack_workspace").unwrap();
    let channel = std::env::var("slack_channel").unwrap();

    let query = String::from_utf8(keyword).unwrap();

    let now = SystemTime::now();
    let dura = now.duration_since(UNIX_EPOCH).unwrap().as_secs() - 3600;
    let url = format!("https://hn.algolia.com/api/v1/search_by_date?tags=story&query={query}&numericFilters=created_at_i>{dura}");

    let mut writer = Vec::new();
    let resp = request::get(url, &mut writer).unwrap();

    if resp.status_code().is_success() {
        let search: Search = serde_json::from_slice(&writer).unwrap();

        let hits = search.hits;
        let list = hits
            .iter()
            .map(|hit| {
                let title = &hit.title;
                let url = &hit.url;
                let object_id = &hit.object_id;
                let author = &hit.author;

                let post = format!("https://news.ycombinator.com/item?id={object_id}");
                let source = match url {
                    Some(u) => format!("(<{u}|source>)"),
                    None => String::new(),
                };

                format!("- *{title}*\n<{post} | post>{source} by {author}\n")
            })
            .collect::<String>();

        let msg = format!(":sparkles: {query} :sparkles:\n{list}");
        send_message_to_channel(&workspace, &channel, msg);
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
