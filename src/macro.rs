#[macro_export]
macro_rules! note {
    ($fmt:literal) => {
        let _ = tokio::io::stderr().write(&format!(concat!($fmt, "\n")).as_bytes()).await?;
    };
    ($fmt:literal, $($code:tt)+) => {
        let _ = tokio::io::stderr().write(&format!(concat!($fmt, "\n"), $($code)*).as_bytes()).await?;
    };
}

#[macro_export]
macro_rules! say {
    ($fmt:literal) => {
        let _ = tokio::io::stdout().write(&format!(concat!($fmt, "\n")).as_bytes()).await?;
    };
    ($fmt:literal, $($code:tt)*) => {
        let _ = tokio::io::stdout().write(&format!(concat!($fmt, "\n"), $($code)*).as_bytes()).await?;
    };
}

#[macro_export]
macro_rules! start_worker {
    ($finder:ident, $path:expr, $func:expr, $fmt:expr) => {
        async move {
            let finder = $finder;

            if let Err(e) = $func(Arc::clone(&finder), $path.clone()).await {
                note!($fmt, $path, e);
            }
            finder.dec_worker_count().await;
            Result::<(), color_eyre::Report>::Ok(())
        }
    };
}
