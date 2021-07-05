#!/usr/bin/env bash
docker network create rs3_network --ip-range=192.168.1.0/24 --subnet=192.168.0.0/16 --gateway=192.168.1.254