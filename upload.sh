#!/bin/bash
cargo clean
./update.sh && cargo zigbuild --target armv7-unknown-linux-gnueabihf.2.26 --release --no-default-features --features "rustls" &&  scp target/armv7-unknown-linux-gnueabihf/release/podsync nas:podsync/podsync.new && cargo clean

