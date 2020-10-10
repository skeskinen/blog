#! /bin/sh
cargo run --bin=file-notify -- --command=cargo --arg=run --arg="--bin=blog" --dir=src --dir=templates --dir=articles --dir=projects --sleep_after_restart_millis=4500
