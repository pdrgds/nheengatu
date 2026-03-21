use std::sync::{atomic::AtomicUsize, Arc};
use tokio::sync::{mpsc, Semaphore};

use crate::{config::Config, services::supabase::SupabaseClient};

pub struct AppState {
    pub config: Config,
    pub translator: Arc<dyn gunnlod_core::translator::Translator + Send + Sync>,
    pub supabase: Arc<SupabaseClient>,
    pub job_semaphore: Arc<Semaphore>,
    pub active_jobs: Arc<AtomicUsize>,
    pub job_tx: mpsc::UnboundedSender<String>,
}
