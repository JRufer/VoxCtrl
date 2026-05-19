use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;
use tracing::{error, info};

use crate::{
    models::{DeliveryResult, OutputTarget},
    targets::{build_target, DeliveryTarget},
};

pub struct OutputTargetRouter {
    targets: Arc<RwLock<HashMap<String, Box<dyn DeliveryTarget>>>>,
}

impl OutputTargetRouter {
    pub fn new(targets: Vec<OutputTarget>) -> Self {
        let map = targets
            .into_iter()
            .map(|t| (t.id.clone(), build_target(t)))
            .collect();
        Self {
            targets: Arc::new(RwLock::new(map)),
        }
    }

    /// Deliver text to the named target. Returns the result, or an error
    /// result if the target id is unknown.
    pub async fn deliver(&self, target_id: &str, text: &str) -> DeliveryResult {
        let guard = self.targets.read().await;
        match guard.get(target_id) {
            Some(target) => {
                let result = target.deliver(text).await;
                if !result.success {
                    error!(
                        target_id,
                        error = ?result.error,
                        "Delivery failed"
                    );
                } else {
                    info!(target_id, bytes = text.len(), "Delivered");
                }
                result
            }
            None => {
                error!(target_id, "Unknown target");
                DeliveryResult::err(format!("Unknown target: {target_id}"))
            }
        }
    }

    /// Hot-reload: replace all targets atomically.
    pub async fn reload(&self, targets: Vec<OutputTarget>) {
        let map: HashMap<_, _> = targets
            .into_iter()
            .map(|t| (t.id.clone(), build_target(t)))
            .collect();
        let mut guard = self.targets.write().await;
        *guard = map;
        info!("Router reloaded ({} targets)", guard.len());
    }

    pub async fn target_ids(&self) -> Vec<String> {
        self.targets.read().await.keys().cloned().collect()
    }
}
