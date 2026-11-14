use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
pub struct CancelOnDrop(pub Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[derive(Debug)]
pub struct StopUnconfirmed;
impl std::fmt::Display for StopUnconfirmed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "git.stop_unconfirmed: Aura 没能确认 Git 进程及其子进程已经停止，它们可能还在运行；写入类操作的结果因此无法确认。它们停下之前，这个仓库的下一次 Git 调用会等着。请稍后用 git.status 核对仓库状态，再决定是否重做；不要自动重试。",
        )
    }
}
impl std::error::Error for StopUnconfirmed {}

#[derive(Debug)]
pub struct GitCancelled;
impl std::fmt::Display for GitCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("git.cancelled: Git 操作在完成前被取消，Git 进程已经停止。")
    }
}
impl std::error::Error for GitCancelled {}
