#!/bin/bash

# 颜色输出函数
print_green() {
    echo -e "\033[32m$1\033[0m"
}

print_red() {
    echo -e "\033[31m$1\033[0m"
}

print_yellow() {
    echo -e "\033[33m$1\033[0m"
}

# 存储失败的文件
failed_files=()

# 测试单个文件的函数
test_config_file() {
    local file=$1
    echo "Testing $file..."
    
    # 运行命令
    RUST_LOG=none,ruci=debug cargo run --features "lua utils use-native-tls quinn steganography lwip smoltcp" -- --log-file "" -c "$file" &
    local pid=$!
    
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

# 列出目录内容并让用户选择
select_directory() {
    local current_dir=$1
    local dirs=()
    local i=1

    echo "Current directory: $current_dir"
    echo "Available directories:"
    
    # 收集所有子目录
    while IFS= read -r dir; do
        if [ -d "$dir" ]; then
            dirs+=("$dir")
            echo "$i) $(basename "$dir")"
            ((i++))
        fi
    done < <(find "$current_dir" -maxdepth 1 -mindepth 1 -type d | sort)

    # 如果没有子目录
    if [ ${#dirs[@]} -eq 0 ]; then
        print_yellow "No subdirectories found in current directory."
        return 1
    fi

    # 让用户选择
    local choice
    while true; do
        read -p "Select a directory (1-${#dirs[@]}, or 'q' to use current directory): " choice
        if [[ "$choice" == "q" ]]; then
            return 1
        elif [[ "$choice" =~ ^[0-9]+$ ]] && [ "$choice" -ge 1 ] && [ "$choice" -le "${#dirs[@]}" ]; then
            selected_dir="${dirs[$((choice-1))]}"
            return 0
        else
            print_red "Invalid choice. Please try again."
        fi
    done
}

# 递归选择目录
select_directory_recursive() {
    local current_dir=$1
    local final_dir=$current_dir

    while true; do
        if select_directory "$final_dir"; then
            final_dir="$selected_dir"
            read -p "Do you want to explore subdirectories of $(basename "$final_dir")? (y/n): " explore
            if [[ "$explore" != "y" ]]; then
                break
            fi
        else
            break
        fi
    done

    echo "Selected directory: $final_dir"
    return_dir="$final_dir"
}

# 获取脚本所在目录的绝对路径
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# 移动到 ruci-cmd 目录
cd "$PROJECT_ROOT/crates/ruci-cmd" || exit 1

# 选择目录
return_dir=""
select_directory_recursive "$PROJECT_ROOT/resource"
selected_dir="$return_dir"

# 让用户选择文件类型
echo "Available file types in $selected_dir:"
file_types=($(find "$selected_dir" -type f -name "*.*" | sed 's/.*\.//' | sort -u))

if [ ${#file_types[@]} -eq 0 ]; then
    print_red "No files found in selected directory."
    exit 1
fi

echo "Found file types:"
for i in "${!file_types[@]}"; do
    echo "$((i+1))) ${file_types[i]}"
done

while true; do
    read -p "Select file type (1-${#file_types[@]}): " type_choice
    if [[ "$type_choice" =~ ^[0-9]+$ ]] && [ "$type_choice" -ge 1 ] && [ "$type_choice" -le "${#file_types[@]}" ]; then
        selected_type="${file_types[$((type_choice-1))]}"
        break
    else
        print_red "Invalid choice. Please try again."
    fi
done

# 获取选定类型的所有文件
config_files=($(find "$selected_dir" -type f -name "*.${selected_type}"))

# 检查文件是否存在
if [ ${#config_files[@]} -eq 0 ]; then
    print_red "No ${selected_type} files found in $selected_dir"
    exit 1
fi

# 主测试逻辑
echo "Starting test with ruci-cmd for ${selected_type} files in $(basename "$selected_dir")..."
echo "----------------------------------------"

RUST_LOG=none,ruci=debug cargo build --features "lua utils use-native-tls quinn steganography lwip smoltcp"

# 测试每个文件
for file in "${config_files[@]}"; do
    # 将文件路径转换为相对于项目根目录的路径
    relative_file="../../${file#"$PROJECT_ROOT/"}"
    test_config_file "$relative_file"
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