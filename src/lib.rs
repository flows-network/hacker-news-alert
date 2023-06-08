use dotenv::dotenv;
use http_req::request;
use openai_flows::{
    chat,
    chat::{ChatModel, ChatOptions},
    OpenAIFlows,
};
use schedule_flows::schedule_cron_job;
use serde_derive::{Deserialize, Serialize};
use slack_flows::send_message_to_channel;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use url;
use article_scraper::Readability;
#[no_mangle]
pub fn run() {
    schedule_cron_job(
        String::from("6 * * * *"),
        "chron job scheduled".to_string(),
        callback,
    );
}

fn callback(keyword: Vec<u8>) {
    dotenv().ok();

    let keyword = std::env::var("KEYWORD").unwrap();
    let workspace = std::env::var("slack_workspace").unwrap();
    let channel = std::env::var("slack_channel").unwrap();

    let query = keyword;

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

async fn get_webpage_text(url: &str) -> Option<String> {
    let parsed_url = Url::parse(url)?;
    let scheme = parsed_url.scheme();
    let host = parsed_url.host_str().unwrap_or("");
    let base_url = Url::parse(&format!("{}://{}", scheme, host)).unwrap();

    let mut writer = Vec::new(); //container for body of a response
    let res = request::get(url, &mut writer).unwrap();

    match Readability::extract(&String::from_utf8(writer).unwrap(), Some(base_url)).await {
        Ok(res) => {
            let output = from_read(res.to_string().as_bytes(), 80);
            Some(output)
        }
        Err(_err) => None,
    }
}

async fn get_summary() {
    let mut openai = OpenAIFlows::new();
    openai.set_retry_times(3);

    let bpe = cl100k_base().unwrap();

    let mut feed_tokens_map = Vec::new();

    let issue_creator_input = format!("User '{issue_creator_name}', who holds the role of '{issue_creator_role}', has submitted an issue titled '{issue_title}', labeled as '{labels}', with the following post: '{issue_body}'.");

    let mut tokens = bpe.encode_ordinary(&issue_creator_input);
    feed_tokens_map.append(&mut tokens);

    match issues_handle.list_comments(issue_number).send().await {
        Ok(pages) => {
            for comment in pages.items {
                let comment_body = comment.body.unwrap_or("".to_string());
                let commenter = comment.user.login;
                let commenter_input = format!("{commenter} commented: {comment_body}");
                let mut tokens = bpe.encode_ordinary(&commenter_input);
                feed_tokens_map.append(&mut tokens);
            }
        }

        Err(_e) => {}
    }

    let chat_id = format!("Issue#{issue_number}");
    let system = &format!("As an AI co-owner of a GitHub repository, you are responsible for conducting a comprehensive analysis of GitHub issues. Your analytic focus encompasses distinct elements, including the issue's title, associated labels, body text, the identity of the issue's creator, their role, and the nature of the comments on the issue. Utilizing these data points, your task is to generate a succinct, context-aware summary of the issue.");

    let co = ChatOptions {
        model: ChatModel::GPT35Turbo,
        restart: true,
        system_prompt: Some(system),
    };

    let total_tokens_count = feed_tokens_map.len();
    let mut _summary = "".to_string();

    if total_tokens_count > 2800 {
        let mut token_vec = feed_tokens_map;
        let mut map_out = "".to_string();

        while !token_vec.is_empty() {
            let drain_to = std::cmp::min(token_vec.len(), 2800);
            let token_chunk = token_vec.drain(0..drain_to).collect::<Vec<_>>();

            let text_chunk = bpe.decode(token_chunk).unwrap();

            let map_question = format!("Given the issue titled '{issue_title}' and a particular segment of body or comment text '{text_chunk}', focus on extracting the central arguments, proposed solutions, and instances of agreement or conflict among the participants. Generate an interim summary capturing the essential information in this section. This will be used later to form a comprehensive summary of the entire discussion.");

            match openai.chat_completion(&chat_id, &map_question, &co).await {
                Ok(r) => {
                    map_out.push_str(&r.choice);
                }
                Err(_e) => {}
            }
        }

        let reduce_question = format!("User '{issue_creator_name}', in the role of '{issue_creator_role}', has filed an issue titled '{issue_title}', labeled as '{labels}'. The key information you've extracted from the issue's body text and comments in segmented form are: {map_out}. Concentrate on the principal arguments, suggested solutions, and areas of consensus or disagreement among the participants. From these elements, generate a concise summary of the entire issue to inform the next course of action.");

        match openai
            .chat_completion(&chat_id, &reduce_question, &co)
            .await
        {
            Ok(r) => {
                _summary = r.choice;
            }
            Err(_e) => {}
        }
    } else {
        let issue_body = bpe.decode(feed_tokens_map).unwrap();

        let question = format!("{issue_body}, concentrate on the principal arguments, suggested solutions, and areas of consensus or disagreement among the participants. From these elements, generate a concise summary of the entire issue to inform the next course of action.");

        match openai.chat_completion(&chat_id, &question, &co).await {
            Ok(r) => {
                _summary = r.choice;
            }
            Err(_e) => {}
        }
    }

    let text = format!("Issue Summary:\n{}\n{}", _summary, issue_url);
    send_message_to_channel(&slack_workspace, &slack_channel, text);
}
