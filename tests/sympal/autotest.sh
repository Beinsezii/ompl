#! /usr/bin/env bash

fp="./$0"
media="${fp%/*}/media"

export RUSTFLAGS=-Awarnings
cmd="cargo run --quiet --no-default-features --features backend-sympal -- --port 54321"

trap "$cmd exit 2> /dev/null; exit 0" INT

EXIT_CODE=0

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
    played=`$cmd print playing 2> /dev/null`
    while [[ `$cmd print playing 2> /dev/null` = true ]]; do
        sleep 0.1
    done
    $cmd exit 2> /dev/null
    if wait $!; then
        if [[ $played = true ]]; then
            echo $file PASS
            continue
        fi
    fi
    echo $file FAIL
    ((EXIT_CODE++))
done

exit $EXIT_CODE
