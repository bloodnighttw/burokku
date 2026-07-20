use runtime::Runtime;

mod window;

const DEFAULT_SCRIPT: &str = r#"
(async () => {
    console.log("A");

    setTimeout(() => console.log("B"), 0);

    Promise.resolve().then(() => console.log("C"));

    console.log("D");

    // Keep the top-level promise alive long enough for the timer macrotask.
    await new Promise(resolve => setTimeout(resolve, 0));
})();
"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run().await
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::new().await?;
    runtime.eval_promise::<()>(DEFAULT_SCRIPT).await?;
    window::run()?;
    Ok(())
}
