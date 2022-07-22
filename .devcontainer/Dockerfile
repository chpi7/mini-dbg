# See here for image contents: https://github.com/microsoft/vscode-dev-containers/tree/v0.241.1/containers/ubuntu/.devcontainer/base.Dockerfile

# [Choice] Ubuntu version (use ubuntu-22.04 or ubuntu-18.04 on local arm64/Apple Silicon): ubuntu-22.04, ubuntu-20.04, ubuntu-18.04
ARG VARIANT="jammy"
FROM mcr.microsoft.com/vscode/devcontainers/base:0-${VARIANT}

RUN apt-get update && export DEBIAN_FRONTEND=noninteractive && \ 
    apt-get -y install --no-install-recommends build-essential

USER vscode
RUN cd ~ && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs > rustup-init.sh && \
    chmod +x rustup-init.sh && \
    ./rustup-init.sh -y && \
    echo 'source $HOME/.cargo/env' >> /home/vscode/.bashrc && \
    echo 'source $HOME/.cargo/env' >> /home/vscode/.zshrc
