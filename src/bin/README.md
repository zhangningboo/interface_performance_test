### MindIE 测试示例
```shell
$ cargo run --bin concurrency_testing -- --requests 40 --concurrency 20 --print-response --url http://ip:port/infer --body '{
    "inputs": "docker容器设置的是开机自启动，容器中有个服务，怎么设置为容器启动后自动启动：nohup mindieservice_daemon > output.log 2>&1 &",
    "stream": false,
    "parameters": {
        "temperature": 0.5,
        "top_k": 10,
        "top_p": 0.95,
        "max_new_tokens": 2048,
        "do_sample": true,
        "seed": 42,
        "repetition_penalty": 1.03,
        "details": true,
        "typical_p": 0.5,
        "watermark": false
    }
}'
```

