use std::hash::Hash;
use std::time::Duration;
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};
use tokio::sync::{oneshot, Mutex};

pub struct Judge<K>
where
    K: Send + Sync + Hash + Eq,
{
    map: Arc<Mutex<HashMap<K, tokio::time::Instant>>>,
    shutdown_t: Option<oneshot::Sender<()>>,

    interval: Duration,
}

impl<K: Send + Sync + Eq + Hash + 'static> Judge<K> {
    pub async fn new(interval: Duration) -> Self {
        let (shutdown_t, mut shutdown_r) = oneshot::channel::<()>();

        let mut ticker = tokio::time::interval(interval);

        let auth_id_map = Arc::new(Mutex::new(HashMap::new()));

        let auth_id_map_c = auth_id_map.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ =&mut shutdown_r => {
                        return
                    }

                    //定时清理过时数据，避免缓存无限增长
                    now = ticker.tick() =>{
                        let mut m = auth_id_map_c.lock().await;
                        m.retain(|_,v|{
                            now.duration_since(*v).lt(&interval)
                        });
                    }
                }
            }
        });

        Self {
            map: auth_id_map,
            shutdown_t: Some(shutdown_t),
            interval,
        }
    }

    pub async fn stop(&mut self) {
        if let Some(st) = self.shutdown_t.take() {
            let _ = st.send(());
        }
    }

    /// return true if the auth_id is ok; return false if it's a replay attack
    ///
    /// if the map doesn't have it, it will store it in the map.
    pub async fn check(&mut self, auth_id: K) -> bool {
        let now = tokio::time::Instant::now();

        let mut m = self.map.lock().await;
        match m.entry(auth_id) {
            Entry::Occupied(occupied_entry) => {
                now.duration_since(*occupied_entry.get()).ge(&self.interval)
            }
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(now);
                true
            }
        }
    }
}
