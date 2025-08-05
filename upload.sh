#!/bin/bash
cargo clean
./update.sh &&  cross build --target armv7-unknown-linux-musleabihf --release --no-default-features --features "rustls" && scp target/armv7-unknown-linux-musleabihf/release/podsync nas:podsync/podsync.new && cargo clean
./update.sh && cargo zigbuild --target armv7-unknown-linux-gnueabihf.2.26 --release --no-default-features --features "rustls" &&  scp target/armv7-unknown-linux-musleabihf/release/podsync nas:podsync/podsync.new && cargo clean

