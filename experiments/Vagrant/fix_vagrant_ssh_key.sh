#!/usr/bin/env bash

echo "Fixing SSH key for vagrant 2.2.16 ..."

# exit when any command fails
set -e
# keep track of the last executed command
trap 'last_command=$current_command; current_command=$BASH_COMMAND' DEBUG
# echo an error message before exiting
trap 'echo "\"${last_command}\" command filed with exit code $?."' EXIT

config="$(vagrant ssh-config)"
host="$(echo "$config" | grep "^  HostName" | cut -d' ' -f4)"
file="$(echo "$config" | grep "^  IdentityFile" | cut -d' ' -f4)"

[ -z "$host"  ] || [ -z "$file"  ] && echo "Failed to get vagrant config" && exit 1

echo "Generating key..."
cat /dev/zero | ssh-keygen -a 100 -t ed25519 -f ubuntu_vagrant -C vagrant -q -N '' || echo "Using existing key"
# cat /dev/zero | ssh-keygen -a 100 -t rsa -b 4096 -f ubuntu_vagrant -C vagrant -q -N '' || echo "Using existing key"

port="$(echo "$config" | grep "^  Port" | cut -d' ' -f4)"

echo "Copying public key..."
ssh-copy-id -o StrictHostKeyChecking=no -o "IdentityFile $file" -f -i ubuntu_vagrant.pub -p $port vagrant@$host 1>/dev/null

echo "Copying private key..."
mv "$file" "$file.bak"
cp ubuntu_vagrant "$file"

echo "Testing connection..."
ssh -o 'BatchMode=yes' -o 'ConnectionAttempts=1' -i "$file" -q -p $port vagrant@$host exit
echo "Connection successful"

echo "Removing temporary keys..."
rm ubuntu_vagrant*

trap 'echo "Success."' EXIT

set +e

