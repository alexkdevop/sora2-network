FROM debian:bullseye
# Install dependencies
RUN apt-get update && \
    apt-get install --no-install-recommends -y \
    ca-certificates nano curl && \
    apt-get autoremove -y && \
    apt-get clean && \
    find /var/lib/apt/lists/ -type f -not -name lock -delete

RUN useradd substrate -u 10000
RUN mkdir /chain && \
    chown 10000:10000 /chain
USER substrate

COPY ./framenode /usr/local/bin/framenode
COPY ./relayer.bin /usr/local/bin/relayer

ENTRYPOINT ["framenode"]