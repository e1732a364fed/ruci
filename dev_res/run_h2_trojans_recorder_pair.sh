#!/bin/bash

RUST_LOG=none,ruci=debug cargo run --features "lua quinn lwip use-native-tls" --example chain -- remote.lua &
PID1=$!

RUST_LOG=none,ruci=debug cargo run --features "lua quinn lwip use-native-tls" --example chain_infinite -- local_mux_h2_recorder.lua &
PID2=$!

sleep 5

kill $PID1

sleep 1

kill $PID2

