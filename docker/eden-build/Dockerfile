FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl git file xz-utils unzip zip \
        build-essential gcc-14 g++-14 cmake ninja-build pkg-config python3 \
        gdb \
        glslang-tools libvulkan-dev spirv-tools \
        libsdl2-dev libssl-dev libusb-1.0-0-dev libudev-dev libasound2-dev libpulse-dev \
        libgl-dev libegl-dev libx11-dev libxext-dev libxrandr-dev \
        libavcodec-dev libavfilter-dev libavutil-dev libswscale-dev \
        zlib1g-dev libzstd-dev liblz4-dev \
        mesa-vulkan-drivers libvulkan1 libgl1 libegl1 libgl1-mesa-dri xvfb \
    && rm -rf /var/lib/apt/lists/*

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        qt6-base-dev qt6-base-private-dev qt6-charts-dev libqt6svg6-dev \
    && rm -rf /var/lib/apt/lists/*

ENV CC=gcc-14 CXX=g++-14

WORKDIR /src
