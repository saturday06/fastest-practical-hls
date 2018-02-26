FROM base/archlinux:2018.02.01
WORKDIR /root
ENV PATH $PATH:/root/.cargo/bin
RUN pacman -Syu --noconfirm clang ffmpeg llvm-libs pkg-config pango ttf-dejavu imagemagick
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain none
COPY rust-toolchain .
RUN rustup install $(cat rust-toolchain)
COPY . .
RUN cargo build --release
ENTRYPOINT ["cargo", "run", "--release"]
