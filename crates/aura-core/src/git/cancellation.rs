use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
pub(crate) struct CancelOnDrop(pub Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[derive(Debug)]
pub(crate) struct StopUnconfirmed;
impl std::fmt::Display for StopUnconfirmed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("git.stop_unconfirmed: Git进程树尚未确认停止，请勿自动重试")
    }
}
impl std::error::Error for StopUnconfirmed {}

#[derive(Debug)]
pub(crate) struct GitCancelled;
impl std::fmt::Display for GitCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("git.cancelled: Git进程树已停止，读取已取消")
    }
}
impl std::error::Error for GitCancelled {}
