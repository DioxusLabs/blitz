use blitz_traits::net::NetCallback;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

pub struct MpscCallback<T>(SyncSender<(usize, T)>);
impl<T> MpscCallback<T> {
    pub fn new() -> (Receiver<(usize, T)>, Self) {
        let (send, recv) = sync_channel(0);
        (recv, Self(send))
    }
}
impl<T: Send + Sync + 'static> NetCallback for MpscCallback<T> {
    type Data = T;
    fn call(&self, doc_id: usize, data: Self::Data) {
        let _ = self.0.send((doc_id, data));
    }
}
