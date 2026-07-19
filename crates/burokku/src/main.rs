use runtime::Runtime;

const DEFAULT_SCRIPT: &str = r#"
(async () => {
    await Promise.resolve();
    return `Hello, Burokku! ${20 + 22}`;
})()
"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run().await
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let source = match std::env::args_os().nth(1) {
        Some(path) => tokio::fs::read_to_string(path).await?,
        None => DEFAULT_SCRIPT.to_owned(),
    };

    let runtime = Runtime::new().await?;
    let result: String = runtime.eval_promise(source).await?;
    println!("{result}");

    Ok(())
}
