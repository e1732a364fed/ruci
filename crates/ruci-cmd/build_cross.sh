#!/bin/bash

features=()
target=""
version=""

while getopts "v:t:f" opt; do
    case $opt in
        v)
            version=($OPTARG)
            ;;
        t)
            target=($OPTARG)
            ;;
        f)
            features+=($OPTARG)
            ;;
        ?)
            echo "Usage: $(basename $0) [-t <target-triple>] [-f features]"
            ;;
    esac
done

TARGET_FEATURES="${features[@]}"

cross build --target ${target} --features "${TARGET_FEATURES}" --release 

mkdir -p dist
cd ../../target/${target}/release && cp ruci-cmd ../../../crates/ruci-cmd/dist/
cd ../../../crates/ruci-cmd && cp -r ../../resource dist/

cd dist && tar -cJf ruci_cmd_${version}_${target}.tar.xz *
