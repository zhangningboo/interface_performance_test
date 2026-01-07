use clap::Parser;
use futures::StreamExt;
use reqwest::Client;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Parser, Debug)]
#[clap(author, version, about = "Concurrency tester for LLM streaming APIs with optional response printing")]
struct Args {
    #[clap(short, long)]
    url: String,

    /// 总请求数
    #[clap(short = 'n', long, default_value_t = 10)]
    requests: usize,

    /// 并发数（最大并发连接数）
    #[clap(short = 'c', long, default_value_t = 10)]
    concurrency: usize,

    /// 请求超时（秒）
    #[clap(short = 't', long, default_value_t = 60)]
    timeout: u64,

    #[clap(short = 'b', long)]
    body: String,

    /// Print the full response body of the first successful request (for debugging)
    #[clap(short = 'p', long)]
    print_response: bool,
}

#[derive(Debug, Clone)]
struct LatencyResult {
    ttft: Duration,
    total: Duration,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()?;

    let (result_sender, mut result_receiver) = mpsc::channel::<LatencyResult>(args.requests);
    let printed = Arc::new(AtomicBool::new(false)); // 保证只打印一次

    println!(
        "Starting benchmark: {} (requests={}, concurrency={})",
        args.url, args.requests, args.concurrency
    );
    if args.print_response {
        println!("ℹ️  Response of the first successful request will be printed below:\n--- RESPONSE START ---");
    }

    let start = Instant::now();

    for _ in 0..args.concurrency {
        let client = client.clone();
        let url = args.url.clone();
        let body = args.body.clone();
        let sender = result_sender.clone();
        let printed = printed.clone();
        let print_enabled = args.print_response;

        tokio::spawn(async move {
            loop {
                let req_start = Instant::now();

                let res = client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    // 注意：不设置 Accept 头（适配 TGI/vLLM）
                    .body(body.clone())
                    .send()
                    .await;

                match res {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            continue;
                        }

                        let mut stream = resp.bytes_stream();
                        let mut should_print = false;

                        // 检查是否需要打印（仅第一个成功请求）
                        if print_enabled && !printed.load(Ordering::Relaxed) {
                            should_print = true;
                            printed.store(true, Ordering::Relaxed);
                        }

                        // 等待第一个 chunk（TTFT）
                        let ttft = match tokio::time::timeout(
                            Duration::from_secs(args.timeout as u64),
                            stream.next(),
                        )
                        .await
                        {
                            Ok(Some(Ok(chunk))) => {
                                if should_print {
                                    // 安全地将 bytes 转为字符串（忽略非法 UTF-8）
                                    let s = String::from_utf8_lossy(&chunk);
                                    print!("{}", s);
                                    std::io::stdout().flush().ok();
                                }
                                req_start.elapsed()
                            }
                            _ => continue,
                        };

                        // 读取剩余流
                        while let Ok(Some(Ok(chunk))) = tokio::time::timeout(
                            Duration::from_millis(100),
                            stream.next(),
                        )
                        .await
                        {
                            if should_print {
                                let s = String::from_utf8_lossy(&chunk);
                                print!("{}", s);
                                std::io::stdout().flush().ok();
                            }
                        }

                        if should_print {
                            println!("\n--- RESPONSE END ---\n");
                        }

                        let total = req_start.elapsed();
                        let _ = sender.send(LatencyResult { ttft, total }).await;
                    }
                    Err(_) => continue,
                }
            }
        });
    }

    // 收集结果
    let mut results = Vec::with_capacity(args.requests);
    for _ in 0..args.requests {
        if let Some(res) = result_receiver.recv().await {
            results.push(res);
        } else {
            break;
        }
    }

    let total_time = start.elapsed();
    let success = results.len();

    println!("\n=== Results ===");
    println!("Total: {}, Success: {}, Failed: {}", args.requests, success, args.requests - success);
    println!("Total time: {:.2?}", total_time);

    if success > 0 {
        let to_ms = |ns: u128| ns as f64 / 1_000_000.0;
        let mut ttfts: Vec<u128> = results.iter().map(|r| r.ttft.as_nanos()).collect();
        let mut totals: Vec<u128> = results.iter().map(|r| r.total.as_nanos()).collect();
        ttfts.sort_unstable();
        totals.sort_unstable();

        let p = |data: &[u128], perc: f64| -> f64 {
            let idx = ((data.len() as f64) * perc).min(data.len() as f64 - 1.0) as usize;
            to_ms(data[idx])
        };

        println!("\n--- TTFT ---");
        println!("Avg: {:.2} ms", ttfts.iter().map(|&x| to_ms(x)).sum::<f64>() / success as f64);
        println!("P50: {:.2} ms", p(&ttfts, 0.5));
        println!("P95: {:.2} ms", p(&ttfts, 0.95));
        println!("P99: {:.2} ms", p(&ttfts, 0.99));

        println!("\n--- End-to-End ---");
        println!("Avg: {:.2} ms", totals.iter().map(|&x| to_ms(x)).sum::<f64>() / success as f64);
        println!("P50: {:.2} ms", p(&totals, 0.5));
        println!("P95: {:.2} ms", p(&totals, 0.95));
        println!("P99: {:.2} ms", p(&totals, 0.99));

        println!("\nRequests/sec: {:.2}", success as f64 / total_time.as_secs_f64());
    }

    let end = Instant::now();
    println!("测试花费时间：{}", end.checked_duration_since(start).unwrap().as_secs_f64());

    Ok(())
}