import socket
import struct
import json
import os
import time
import hashlib
from tailsync.constants import TCP_PORT, BASE_DIR

def start_network_server(manager):
    """后台监听服务[cite: 1]"""
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    
    try:
        server.bind(("0.0.0.0", TCP_PORT))
        server.listen(5)
    except Exception as e:
        print(f"❌ 服务器绑定失败: {e}")
        return

    while True:
        try:
            client, addr = server.accept()
            mode = client.recv(1)
            if not mode: continue

            # 读取长度头
            header_data = bytearray()
            while len(header_data) < 4:
                chunk = client.recv(4 - len(header_data))
                if not chunk: break
                header_data.extend(chunk)
            
            if len(header_data) < 4:
                client.close()
                continue
                
            data_len = struct.unpack('!I', header_data)[0]

            if mode in [b'\x00', b'\x01']: # 处理文本或图片
                payload = bytearray()
                while len(payload) < data_len:
                    chunk = client.recv(min(data_len - len(payload), 1024*64))
                    if not chunk: break
                    payload.extend(chunk)
                manager.remote_update_signal.emit(ord(mode), bytes(payload))

            elif mode == b'\x02': # 处理文件接收
                header_json = bytearray()
                while len(header_json) < data_len:
                    chunk = client.recv(data_len - len(header_json))
                    header_json.extend(chunk)
                
                info = json.loads(header_json.decode())
                file_size, file_name = info['size'], info['name']
                save_path = os.path.join(BASE_DIR, f"recv_{time.strftime('%H%M%S')}_{file_name}")
                
                received = 0
                manager.transfer_cancelled = False 
                
                with open(save_path, "wb") as f:
                    while received < file_size:
                        if manager.transfer_cancelled: break
                        chunk = client.recv(min(file_size - received, 128*1024))
                        if not chunk: break
                        f.write(chunk)
                        received += len(chunk)
                        if file_size > 5 * 1024 * 1024:
                            manager.progress_signal.emit(int(received/file_size*100), "📥 接收中...", file_name, False)
                
                if not manager.transfer_cancelled:
                    # 【核心修复】接收完成后直接设置屏蔽盾
                    manager.blocked_filename = file_name
                    
                    if file_size > 5 * 1024 * 1024:
                        manager.progress_signal.emit(100, "✅ 完成", file_name, True)
                    
                    manager.remote_update_signal.emit(2, save_path)
            
            client.close()
        except Exception as e:
            print(f"⚠️ 网络接收异常: {e}")