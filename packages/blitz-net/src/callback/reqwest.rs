use blitz_traits::net::NetCallback;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

pub struct MpscCallback<T>(UnboundedSender<(usize, T)>);
impl<T> MpscCallback<T> {
    pub fn new() -> (UnboundedReceiver<(usize, T)>, Self) {
        let (send, recv) = unbounded_channel();
        (recv, Self(send))
    }
}
impl<T: Send + Sync + 'static> NetCallback for MpscCallback<T> {
    type Data = T;
    fn call(&self, doc_id: usize, data: Self::Data) {
        let _ = self.0.send((doc_id, data));
    }
}
