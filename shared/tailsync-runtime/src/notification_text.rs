//! Content-free text for the existing native notification paths.

pub enum SystemNotification<'a> {
    Arrival {
        kind: &'a str,
        peer: &'a str,
        count: usize,
    },
    FilesReceived {
        count: usize,
    },
    ClipboardUpdateFailed,
    FileTransferFailed,
    StorageUnavailable,
    StorageRecovered,
    StorageMoving,
    NotificationGap {
        count: u64,
    },
}

impl SystemNotification<'_> {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Arrival { .. } => "arrival",
            Self::FilesReceived { .. } => "filesReceived",
            Self::ClipboardUpdateFailed => "clipboardUpdateFailed",
            Self::FileTransferFailed => "fileTransferFailed",
            Self::StorageUnavailable => "storageUnavailable",
            Self::StorageRecovered => "storageRecovered",
            Self::StorageMoving => "storageMoving",
            Self::NotificationGap { .. } => "notificationGap",
        }
    }
    /// Language is the already-validated local Settings language; no clipboard body or error detail is accepted.
    pub fn body(&self, language: &str) -> String {
        let chinese = language == "zh-CN";
        match self {
            Self::NotificationGap { count } => {
                if chinese {
                    format!("有 {count} 条较早的通知已不可读取，请查看历史记录和传输状态。")
                } else {
                    format!("{count} earlier notifications are no longer available. Check history and transfer status.")
                }
            }
            Self::Arrival { kind, peer, count } => {
                if *count > 1 {
                    return if chinese {
                        format!("已收到来自 {peer} 的 {count} 项内容")
                    } else {
                        format!("Received {count} items from {peer}")
                    };
                }
                let content = match (*kind, chinese) {
                    ("text", true) => "文本",
                    ("image", true) => "图片",
                    ("file", true) => "文件",
                    (_, true) => "内容",
                    ("text", false) => "Text",
                    ("image", false) => "Image",
                    ("file", false) => "File",
                    (_, false) => "Content",
                };
                if chinese {
                    format!("来自 {peer} 的{content}")
                } else {
                    format!("{content} from {peer}")
                }
            }
            Self::FilesReceived { count } => {
                if chinese {
                    format!("已收到 {count} 个文件")
                } else if *count == 1 {
                    "Received 1 file".into()
                } else {
                    format!("Received {count} files")
                }
            }
            Self::ClipboardUpdateFailed => if chinese {
                "内容已保存，但未能更新剪贴板。"
            } else {
                "Content was saved, but the clipboard could not be updated."
            }
            .into(),
            Self::FileTransferFailed => if chinese {
                "文件传输未能完成，请查看传输状态。"
            } else {
                "File transfer could not be completed. Check the transfer status."
            }
            .into(),
            Self::StorageUnavailable => if chinese {
                "存储暂不可用，文件传输已暂停。"
            } else {
                "Storage is unavailable. File transfer is paused."
            }
            .into(),
            Self::StorageRecovered => if chinese {
                "存储已恢复，文件传输已继续。"
            } else {
                "Storage is available again. File transfer resumed."
            }
            .into(),
            Self::StorageMoving => if chinese {
                "文件传输已完成，正在移动 TailSync 数据。"
            } else {
                "File transfers finished. Moving TailSync data now."
            }
            .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_text_contains_only_type_peer_and_counts() {
        let arrival = SystemNotification::Arrival {
            kind: "text",
            peer: "MacBook",
            count: 1,
        };
        assert_eq!(arrival.body("zh-CN"), "来自 MacBook 的文本");
        assert_eq!(arrival.body("en"), "Text from MacBook");
        assert_eq!(
            SystemNotification::FilesReceived { count: 1 }.body("en"),
            "Received 1 file"
        );
        assert_eq!(
            SystemNotification::FilesReceived { count: 10 }.body("zh-CN"),
            "已收到 10 个文件"
        );
    }
}
