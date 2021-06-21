extern crate futures;
extern crate reqwest;
extern crate select;

use futures::stream::futures_unordered::FuturesUnordered;
use futures::stream::StreamExt;
use select::document::Document;
use select::predicate::Name;

use reqwest::{Client, Error, Response};
use scraper::{Html, Node, Selector};
use std::fs;

// Used links:
// https://stackoverflow.com/questions/59237895/how-to-process-a-vector-as-an-asynchronous-stream

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url = "https://docs.aws.amazon.com/AmazonS3/latest/API/API_Operations.html";
    let client = reqwest::Client::new();
    let resp = client.get(base_url).send().await?;
    let body = resp.text().await?;
    // let base_document = Html::parse_document(&body);
    let base_document = Document::from(body.as_str());
    let responses: Vec<Result<Response, Error>> = base_document
        .find(Name("a"))
        .filter_map(|n| n.attr("href"))
        .map(|s| s.replace("./", ""))
        .filter(|s| s.starts_with("API_"))
        .map(|mut s| {
            s.insert_str(0, base_url.replace("API_Operations.html", "").as_str());
            s
        })
        .map(|l| get_link(l, &client))
        .collect::<FuturesUnordered<_>>()
        .collect::<Vec<_>>()
        .await;

    let mut documents: Vec<Vec<String>> = Vec::new();
    for response in responses {
        let body = response.unwrap().text().await?;
        let api_document = Html::parse_document(&body);
        let selector = Selector::parse("div > h1, div > h2, div > pre > code").unwrap();
        let title_selector = Selector::parse("h1").unwrap();
        let header_selector = Selector::parse("h2").unwrap();
        let code_selector = Selector::parse("code").unwrap();
        let mut document: Vec<String> = Vec::new();
        for element in api_document.select(&selector) {
            let api_fragments = Html::parse_fragment(&element.html());
            for fragment in api_fragments.select(&title_selector) {
                let title = format!("{:?}", fragment.inner_html()).replace("\"", "");
                document.push(title);
                // println!("{}", title);
            }
            for fragment in api_fragments.select(&header_selector) {
                let header = format!("{:?}", fragment.inner_html()).replace("\"", "");
                document.push(header);
                // println!("{}", header);
            }
            for fragment in api_fragments.select(&code_selector) {
                let code = format!("{:?}", fragment.inner_html())
                    .replace("\"", "")
                    .replace("\\n", "\n")
                    // .replace("&lt;", "<")
                    // .replace("&gt;", ">")
                    // .replace("&amp;", "&")
                    .replace("\\", "");
                document.push(code);
                // println!("{}", code);
            }
            // println!("{}", "---------------");
        }
        documents.push(document);
        // println!("{}", "***************");
    }
    let mut final_output = String::new();
    for document in documents.iter() {
        final_output.push_str("*******************");
        final_output.push('\n');
        // println!("{}", "*******************");
        // println!("{}", document[0]);
        final_output.push_str(document[0].as_str());
        final_output.push('\n');
        // println!("{}", document[1]);
        // println!("{}", document[2]);
        let lines = document[2].split("\n");
        for line in lines {
            let line_fragment = Html::parse_fragment(line);
            for node in line_fragment.tree {
                if let Node::Text(text) = node {
                    // print!("{}", text.text.replace("<?", "\n<?"));
                    final_output.push_str(text.text.replace("<?", "\n<?").as_str());
                }
            }
            final_output.push('\n');
            // println!("{}", "");
        }
    }
    fs::write("./apirequests.txt", final_output.clone()).expect("Unable to write to file");
    //println!("{}", final_output);
    Ok(())
}

async fn get_link(url: String, client: &Client) -> Result<Response, Error> {
    client.get(url).send().await
}
