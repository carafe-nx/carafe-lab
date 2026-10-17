FROM carafe-lab-eden-build

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        xdotool imagemagick x11vnc novnc websockify x11-utils mesa-utils vulkan-tools \
    && rm -rf /var/lib/apt/lists/*

COPY --from=carafe-lab-mesa-d3d12 /opt/mesa-d3d12 /opt/mesa-d3d12
COPY --from=carafe-lab-mesa-d3d12 /opt/carafe /opt/carafe
COPY --from=scripts xwatch.c /tmp/xwatch.c
RUN gcc -O2 -Wall -o /usr/local/bin/xwatch /tmp/xwatch.c -lX11 -lXext && rm /tmp/xwatch.c
COPY eden /opt/eden/bin/eden
COPY --from=scripts run-eden.sh /usr/local/bin/run-eden
COPY --from=scripts carafe.html /usr/share/novnc/carafe.html
RUN chmod +x /opt/eden/bin/eden /usr/local/bin/run-eden \
    && echo "v0.2.1+carafe-lab $(uname -m)" > /opt/eden/carafe-version

WORKDIR /work
ENTRYPOINT ["/usr/local/bin/run-eden"]
