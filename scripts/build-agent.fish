#!/usr/bin/fish
# This script is for development purposes
docker build services/judge-core --file ./services/judge-core/Dockerfile.agent --tag judge-core:latest
docker image save judge-core:latest -o judgecore.tar
sudo ctr -n judge-core image import judgecore.tar
rm judgecore.tar
