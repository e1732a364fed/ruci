#!/bin/bash

# 颜色输出函数
print_green() {
    echo -e "\033[32m$1\033[0m"
}

print_red() {
    echo -e "\033[31m$1\033[0m"
}

# 存储失败的文件
failed_files=()

# 测试单个文件的函数
test_lua_file() {
    local file=$1
    echo "Testing $file..."
    
    # 运行命令
    RUST_LOG=none,ruci=debug cargo run --features "lua quinn tun smoltcp use-native-tls steganography" --example lua "../$file" &
    local pid=$!
    
    # 等待2秒
    sleep 2
    
    # 检查进程是否仍在运行
    if ps -p $pid > /dev/null; then
        print_green "✓ $file is running correctly"
        # 终止进程
        kill $pid
        wait $pid 2>/dev/null
    else
        print_red "✗ $file failed to run"
        failed_files+=("$file")
    fi
    
    # 确保清理所有相关进程
    pkill -P $pid 2>/dev/null
    echo "----------------------------------------"
}

# 获取脚本所在目录的绝对路径
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# 移动到 rucimp 目录
cd "$SCRIPT_DIR/../rucimp" || exit 1

# 主测试逻辑
echo "Starting Lua examples test..."
echo "----------------------------------------"

# 获取所有lua文件的列表
lua_files=("../resource/lua_examples/local/"*.lua)

# 检查文件是否存在
if [ ! -e "${lua_files[0]}" ]; then
    print_red "No Lua files found in ../resource/lua_examples/local/"
    exit 1
fi

# 测试每个文件
for file in "${lua_files[@]}"; do
    # 将文件路径转换为相对于项目根目录的路径
    relative_file=${file#"../"}
    test_lua_file "$relative_file"
done

# 输出结果摘要
echo "Test Summary:"
if [ ${#failed_files[@]} -eq 0 ]; then
    print_green "All files passed!"
else
    print_red "The following files failed:"
    for file in "${failed_files[@]}"; do
        print_red "- $file"
    done
    print_red "Total failed: ${#failed_files[@]}"
fi 