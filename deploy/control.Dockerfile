FROM ubuntu:24.04@sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852
LABEL org.opencontainers.image.title="Boxoro native Rust control plane"
LABEL org.opencontainers.image.licenses="MIT"
COPY agent-rush /usr/local/bin/agent-rush
COPY ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY ca-certificates-copyright /usr/share/doc/ca-certificates/copyright
COPY --chown=65532:65532 state/ /data/
USER 65532:65532
WORKDIR /data
ENV AR_DATA_DIR=/data AR_BIND=0.0.0.0:8080 AR_SECURE_COOKIES=true
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/agent-rush"]
CMD ["serve"]
