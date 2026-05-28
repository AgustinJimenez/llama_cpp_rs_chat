use std::sync::Arc;

use super::*;

fn create_test_db() -> Arc<Database> {
    Arc::new(Database::new(":memory:").unwrap())
}

#[test]
fn test_create_conversation() {
    let db = create_test_db();
    let id = db.create_conversation().unwrap();
    assert!(id.starts_with("chat_"));
    assert!(db.conversation_exists(&id).unwrap());
}

#[test]
fn test_insert_and_get_messages() {
    let db = create_test_db();
    let conv_id = db.create_conversation().unwrap();

    db.insert_message(&conv_id, "user", "Hello", 1234567890, 0)
        .unwrap();
    db.insert_message(&conv_id, "assistant", "Hi there!", 1234567891, 1)
        .unwrap();

    let messages = db.get_messages(&conv_id).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[0].content, "Hello");
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[1].content, "Hi there!");
}

#[test]
fn test_conversation_logger() {
    let db = create_test_db();
    let mut logger = ConversationLogger::new(db.clone(), Some("System prompt")).unwrap();

    logger.log_message("USER", "Hello");
    logger.start_assistant_message();
    logger.log_token("Hi ");
    logger.log_token("there!");
    logger.finish_assistant_message();

    let text = logger.get_full_conversation();
    assert!(text.contains("SYSTEM:\nSystem prompt"));
    assert!(text.contains("USER:\nHello"));
    assert!(text.contains("ASSISTANT:\nHi there!"));
}

#[test]
fn test_delete_conversation() {
    let db = create_test_db();
    let id = db.create_conversation().unwrap();
    db.insert_message(&id, "user", "Test", 0, 0).unwrap();

    assert!(db.conversation_exists(&id).unwrap());
    db.delete_conversation(&id).unwrap();
    assert!(!db.conversation_exists(&id).unwrap());
}
