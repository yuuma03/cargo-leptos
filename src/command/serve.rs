use crate::config::Config;
use crate::ext::anyhow::Result;
use crate::service::serve;

pub async fn serve(conf: &Config) -> Result<()> {
    super::build::build_all(conf).await?;
    let default_run = conf.current_project()?;
    let server = serve::spawn(&default_run).await;
    server.await??;
    Ok(())
}
