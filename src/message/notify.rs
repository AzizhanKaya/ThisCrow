use crate::State;
use crate::id::id;
use crate::message::Message;
use crate::msgpack;
use bytes::Bytes;
use serde::Serialize;
use std::borrow::Borrow;
use std::collections::HashSet;

pub trait NotifyIterExt {
    fn notify<T: Serialize>(self, message: Message<T>, state: &State);
}

impl<I> NotifyIterExt for I
where
    I: Iterator,
    I::Item: Borrow<id>,
{
    fn notify<T: Serialize>(self, message: Message<T>, state: &State) {
        let message = Bytes::from(msgpack!(message));

        self.filter_map(|user_id| state.users.get(user_id.borrow()))
            .for_each(|user| user.send_bytes(message.clone()));
    }
}

pub trait NotifyCollectionExt {
    fn notify<T: Serialize>(&self, message: Message<T>, state: &State);
}

impl NotifyCollectionExt for [id] {
    fn notify<T: Serialize>(&self, message: Message<T>, state: &State) {
        let message = Bytes::from(msgpack!(message));

        self.iter()
            .filter_map(|user_id| state.users.get(user_id))
            .for_each(|user| user.send_bytes(message.clone()));
    }
}

impl NotifyCollectionExt for Vec<id> {
    fn notify<T: Serialize>(&self, message: Message<T>, state: &State) {
        self.as_slice().notify(message, state);
    }
}

impl NotifyCollectionExt for HashSet<id> {
    fn notify<T: Serialize>(&self, message: Message<T>, state: &State) {
        self.iter().notify(message, state);
    }
}
