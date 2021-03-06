//! Contains structure which provides futures::Stream to websocket-feed of Coinbase api

extern crate url;

use self::url::Url;
use futures::{future, Sink, Stream};
use futures_util::{
    future::{FutureExt, TryFutureExt},
    sink::SinkExt,
    stream::{StreamExt, TryStreamExt},
};
use hyper::Method;
use serde_json;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_tungstenite::connect_async;

use super::tokio_tungstenite::tungstenite::Message as TMessage;
use crate::error::WSError;
use crate::structs::wsfeed::*;
use crate::{private::Private, ASync};

pub struct WSFeed;

fn convert_msg(msg: TMessage) -> Message {
    match msg {
        TMessage::Text(str) => serde_json::from_str(&str).unwrap_or_else(|e| {
            Message::InternalError(WSError::Serde {
                error: e,
                data: str,
            })
        }),
        _ => unreachable!(), // filtered in stream
    }
}

impl WSFeed {
    // Constructor for simple subcription with product_ids and channels
    pub fn new(
        uri: &str,
        product_ids: &[&str],
        channels: &[ChannelType],
    ) -> impl Stream<Item = Result<Message, WSError>> {
        let subscribe = Subscribe {
            _type: SubscribeCmd::Subscribe,
            product_ids: product_ids.into_iter().map(|x| x.to_string()).collect(),
            channels: channels
                .to_vec()
                .into_iter()
                .map(|x| Channel::Name(x))
                .collect::<Vec<_>>(),
            auth: None,
        };

        Self::new_with_sub(uri, subscribe)
    }

    // Constructor for extended subcription via Subscribe structure
    pub fn new_with_sub(
        uri: &str,
        subsribe: Subscribe,
    ) -> impl Stream<Item = Result<Message, WSError>> {
        let url = Url::parse(uri).unwrap();

        let stream = connect_async(url).map_err(WSError::Connect);
        let stream = {
            stream.and_then(|(ws_stream, _)| async move {
                debug!("WebSocket handshake has been successfully completed");
                let (mut sink, stream) = ws_stream.split();

                let subsribe = serde_json::to_string(&subsribe).unwrap();

                let ret = sink
                    .send(TMessage::Text(subsribe))
                    .map_err(WSError::Send)
                    .await;
                debug!("subsription sent");
                ret.and_then(|_| {
                    Ok(stream
                        .try_filter(|msg| future::ready(msg.is_text()))
                        .map_ok(convert_msg)
                        .map_err(WSError::Read))
                })
            })
        };
        stream.try_flatten_stream()
    }

    // Constructor for simple subcription with product_ids and channels with auth
    pub fn new_with_auth(
        uri: &str,
        product_ids: &[&str],
        channels: &[ChannelType],
        key: &str,
        secret: &str,
        passphrase: &str,
    ) -> impl Stream<Item = Result<Message, WSError>> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("leap-second")
            .as_secs();

        let signature =
            Private::<ASync>::sign(secret, timestamp, Method::GET, "/users/self/verify", "");

        let auth = Auth {
            signature,
            key: key.to_string(),
            passphrase: passphrase.to_string(),
            timestamp: timestamp.to_string(),
        };

        let subscribe = Subscribe {
            _type: SubscribeCmd::Subscribe,
            product_ids: product_ids.into_iter().map(|x| x.to_string()).collect(),
            channels: channels
                .to_vec()
                .into_iter()
                .map(|x| Channel::Name(x))
                .collect::<Vec<_>>(),
            auth: Some(auth),
        };

        Self::new_with_sub(uri, subscribe)
    }
}
