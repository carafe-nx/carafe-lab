FROM carafe-lab-eden-build

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        meson-1.7 python3-mako python3-yaml python3-packaging directx-headers-dev \
        libdrm-dev libexpat1-dev flex bison libglvnd-dev libx11-xcb-dev libxfixes-dev \
        libxcb-glx0-dev libxcb-dri2-0-dev libxcb-dri3-dev libxcb-present-dev libxcb-shm0-dev \
        libxcb-randr0-dev libxcb-sync-dev libxcb-xfixes0-dev libxshmfence-dev libxxf86vm-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
