use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await
}

// tokio's timer doesn't run on wasm32-unknown-unknown
#[cfg(target_arch = "wasm32")]
pub async fn sleep(duration: Duration) {
    gloo_timers::future::sleep(duration).await
}
