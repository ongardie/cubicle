#!/bin/sh
set -eux
cd
puppeteer browsers install chrome-headless-shell
tar --create --file provides.tar .cache/puppeteer
