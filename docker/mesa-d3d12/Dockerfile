FROM carafe-lab-eden-build AS build

COPY build.sh mesa-d3d12-query-recursion.patch carafe-sigchain.c /fix/
RUN sh /fix/build.sh

FROM scratch
COPY --from=build /opt/mesa-d3d12 /opt/mesa-d3d12
COPY --from=build /opt/carafe /opt/carafe
