# FASTEST PRACTICAL HLS

最速のHLS配信サーバーを目指します。Macで動かすとレスポンス返ってこないことがあるのでデッドロックがある気がします。

## 試し方

サーバー起動

```
docker build . -t fastest-practical-hls && docker run -i -p 3001:3001 fastest-practical-hls
```

下記のURLをブラウザで閲覧

```
http://localhost:3001
```
