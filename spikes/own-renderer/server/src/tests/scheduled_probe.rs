//! Test-only commit seam delegation, shared by PG and materializer cases.
use super::*;
/// Test-only delegation at the host/adapter seam. The real adapter still owns
/// its transaction and queue token; no candidate is rebound to a fake source.
struct CommitProbe {
    inner: Arc<dyn data::DataLayer>,
    before: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    lose_ack: bool,
    hide_receipt: bool,
    crossed: Arc<std::sync::atomic::AtomicUsize>,
}
impl data::DataLayer for CommitProbe {
    fn reads(&self, s: &str, stop: Option<Stopped>) -> data::Ops {
        self.inner.reads(s, stop)
    }
    fn begin<'a>(&'a self, s: &str) -> Box<dyn data::Staged + 'a> {
        Box::new(ProbeStaged {
            inner: self.inner.begin(s),
            probe: self,
        })
    }
    fn grants(&self) -> Vec<&'static str> {
        self.inner.grants()
    }
    fn default_page(&self) -> Option<&'static str> {
        self.inner.default_page()
    }
    fn session_entry(&self) -> bool {
        self.inner.session_entry()
    }
    fn provides(&self) -> Result<data::Provided, String> {
        self.inner.provides()
    }
    fn keeps_interactions(&self) -> bool {
        self.inner.keeps_interactions()
    }
    fn recalled(&self, k: &str, since: u64) -> Result<Option<(String, String)>, String> {
        if self.hide_receipt && k.contains("schedule-") {
            Ok(None)
        } else {
            self.inner.recalled(k, since)
        }
    }
    fn runs(&self) -> Option<&dyn schedules::Runs> {
        self.inner.runs()
    }
    fn tail(&self) -> Option<&crate::hosts::Tail> {
        self.inner.tail()
    }
    fn instance(&self) -> Option<u64> {
        self.inner.instance()
    }
}
struct ProbeStaged<'a> {
    inner: Box<dyn data::Staged + 'a>,
    probe: &'a CommitProbe,
}
impl data::Staged for ProbeStaged<'_> {
    fn ops(&self) -> data::Ops {
        self.inner.ops()
    }
    fn rows(&self) -> Option<Vec<(String, String)>> {
        self.inner.rows()
    }
    fn projection_scope(&self) -> data::ProjectionScope {
        self.inner.projection_scope()
    }
    fn publish(&mut self) {
        self.inner.publish();
    }
    fn prepare_materialized(&mut self) -> Result<(), String> {
        self.inner.prepare_materialized()
    }
    fn commit(
        &mut self,
        e: &[data::Handed],
        i: &[data::Dropped],
        r: Option<&data::Recorded<'_>>,
        s: &[schedules::Scheduled],
        c: Option<schedules::Completing<'_>>,
    ) -> Result<data::Commit, String> {
        if c.is_some()
            && let Some(before) = self.probe.before.lock().expect("probe hook").take()
        {
            before();
        }
        let outcome = self.inner.commit(e, i, r, s, c)?;
        if c.is_some() && matches!(outcome, data::Commit::Applied(_)) {
            self.probe
                .crossed
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.probe.lose_ack {
                return Err("test seam lost acknowledgement after actual COMMIT".into());
            }
        }
        Ok(outcome)
    }
}
pub(super) fn probe(
    s: &mut Server,
    before: Option<Box<dyn FnOnce() + Send>>,
    lose_ack: bool,
    hide_receipt: bool,
) -> Arc<std::sync::atomic::AtomicUsize> {
    let crossed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    s.data = Arc::new(CommitProbe {
        inner: s.data.clone(),
        before: Mutex::new(before),
        lose_ack,
        hide_receipt,
        crossed: crossed.clone(),
    });
    crossed
}
