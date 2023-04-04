#!/bin/bash
cargo clean
git pull --rebase origin master &&  cross build --target armv7-unknown-linux-musleabihf --release --no-default-features --features "rustls" && scp target/armv7-unknown-linux-musleabihf/release/podsync nas:podsync/podsync.new && cargo clean
