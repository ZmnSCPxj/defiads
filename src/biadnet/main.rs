//
// Copyright 2019 Tamas Blummer
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//

#[macro_use]extern crate log;

use bitcoin::{
    Block, BlockHeader,
    network::constants::Network
};
use murmel::{
    dispatcher::Dispatcher,
    p2p::P2P
};

use log::Level;
use simple_logger::init_with_level;
use std::sync::mpsc;
use murmel::p2p::{PeerSource, PeerMessageSender};
use murmel::p2p::P2PControl;
use std::net::SocketAddr;
use std::net::SocketAddrV4;
use std::net::Ipv4Addr;
use std::path::Path;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use future::Future;
use futures::{Never, future};
use futures::{Poll,Async};
use futures::task;
use futures::executor::{Executor, ThreadPoolBuilder};
use murmel::chaindb::ChainDB;
use murmel::headerdownload::HeaderDownload;
use murmel::timeout::Timeout;
use murmel::downstream::Downstream;
use murmel::dns::dns_seed;
use rand::{thread_rng, RngCore};

use biadne::store::ContentStore;
use std::sync::RwLock;
use biadne::error::BiadNetError;
use murmel::error::MurmelError;

const MAX_PROTOCOL_VERSION: u32 = 70001;

pub fn main () {
    simple_logger::init_with_level(Level::Debug).unwrap();

    let (sender, receiver) = mpsc::sync_channel(100);

    let mut dispatcher = Dispatcher::new(receiver);

    let chaindb = Arc::new(RwLock::new(
        ChainDB::new(&Path::new("headers"), Network::Bitcoin, 0).expect("can not open db")));
    chaindb.write().unwrap().init(false).expect("can not initialize db");

    let (p2p, p2p_control) = P2P::new(
        "biadnet 0.1.0".to_string(),
        Network::Bitcoin,
        0,
        MAX_PROTOCOL_VERSION,
        false,
        PeerMessageSender::new(sender),
        10);

    let timeout = Arc::new(Mutex::new(Timeout::new(p2p_control.clone())));

    let downstream = Arc::new(Mutex::new(Driver{store: ContentStore::new()}));

    let header_downloader = HeaderDownload::new(chaindb.clone(), p2p_control.clone(), timeout, downstream);

    dispatcher.add_listener(header_downloader);

    let mut thread_pool = ThreadPoolBuilder::new().create().expect("can not start thread pool");
    let p2p2 = p2p.clone();
    let p2p_task = Box::new(future::poll_fn(move |ctx| {
        p2p2.run(0, ctx).unwrap();
        Ok(Async::Ready(()))
    }));
    // start the task that runs all network communication
    thread_pool.spawn(p2p_task).unwrap();

    // note that this call does not return
    thread_pool.run(keep_connected(p2p.clone(), vec!(), 3)).unwrap();
}

pub struct Driver {
    store: ContentStore
}

impl Downstream for Driver {
    fn block_connected(&mut self, block: &Block, height: u32) {}

    fn header_connected(&mut self, block: &BlockHeader, height: u32) {
        self.store.add_header(block).expect("can not add header");
    }

    fn block_disconnected(&mut self, _: &BlockHeader) {
        self.store.unwind_tip().expect("can not unwind tip");
    }
}

fn keep_connected(p2p: Arc<P2P>, peers: Vec<SocketAddr>, min_connections: usize) -> Box<Future<Item=(), Error=Never> + Send> {

    // add initial peers if any
    let mut added = Vec::new();
    for addr in &peers {
        added.push(p2p.add_peer(PeerSource::Outgoing(addr.clone())));
    }

    struct KeepConnected {
        min_connections: usize,
        connections: Vec<Box<Future<Item=SocketAddr, Error=MurmelError> + Send>>,
        p2p: Arc<P2P>,
        dns: Vec<SocketAddr>,
        earlier: HashSet<SocketAddr>
    }

    // this task runs until it runs out of peers
    impl Future for KeepConnected {
        type Item = ();
        type Error = Never;

        fn poll(&mut self, cx: &mut task::Context) -> Poll<Self::Item, Self::Error> {
            // return from this loop with 'pending' if enough peers are connected
            loop {
                // add further peers from db if needed
                self.peers_from_db();
                self.dns_lookup();

                if self.connections.len() == 0 {
                    // run out of peers. this is fatal
                    error!("no more peers to connect");
                    return Ok(Async::Ready(()));
                }
                // find a finished peer
                let finished = self.connections.iter_mut().enumerate().filter_map(|(i, f)| {
                    // if any of them finished
                    // note that poll is reusing context of this poll, so wakeups come here
                    match f.poll(cx) {
                        Ok(Async::Pending) => None,
                        Ok(Async::Ready(e)) => {
                            trace!("woke up to lost peer");
                            Some((i, Ok(e)))
                        }
                        Err(e) => {
                            trace!("woke up to peer error");
                            Some((i, Err(e)))
                        }
                    }
                }).next();
                match finished {
                    Some((i, _)) => self.connections.remove(i),
                    None => return Ok(Async::Pending)
                };
            }
        }
    }

    impl KeepConnected {
        fn peers_from_db(&mut self) {
            // TODO
        }

        fn dns_lookup(&mut self) {
            while self.connections.len() < self.min_connections {
                if self.dns.len() == 0 {
                    self.dns = dns_seed(self.p2p.network);
                }
                if self.dns.len() > 0 {
                    let mut rng = thread_rng();
                    let addr = self.dns[(rng.next_u64() as usize) % self.dns.len()];
                    self.connections.push(self.p2p.add_peer(PeerSource::Outgoing(addr)));
                }
            }
        }
    }

    Box::new(KeepConnected { min_connections, connections: added, p2p, dns: Vec::new(), earlier: HashSet::new() })
}