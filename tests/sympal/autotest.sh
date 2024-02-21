#! /usr/bin/env bash

fp="./$0"
media="${fp%/*}/media"

export RUSTFLAGS=-Awarnings
cmd="cargo run --quiet --no-default-features --features backend-sympal -- --port 54321"

SUCCESS=true

$cmd exit 2> /dev/null
for file in $media/*
do
    $cmd main -dR --backend sympal &
    while [ -z `$cmd print status 2> /dev/null` ]
    do
        sleep 0.1
    done
    $cmd play-file "$file"
    sleep 0.1
    while [[ `$cmd print playing 2> /dev/null` = true ]]; do
        sleep 0.1
    done
    $cmd exit 2> /dev/null
    if wait $!
    then
        echo $file PASS
    else
        echo $file FAIL
        SUCCESS=false
    fi
done

[[ $SUCCESS = true ]] && exit 0 || exit 1
