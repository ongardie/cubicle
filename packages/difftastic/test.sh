#!/bin/sh
set -eu

echo hi > x
echo bye > y

output=$(difft x y)
echo "$output" | grep hi
echo "$output" | grep bye

echo hi > y
output=$(difft x y)
echo "$output" | grep "No changes"
