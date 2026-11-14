use aura_desktop_domain::safety::payment_label;

#[test]
fn payment_actions_match_english_chinese_and_automation_ids_but_history_is_readable() {
    for text in [
        "确认付款",
        "转账",
        "立即购买",
        "完成交易",
        "Buy now",
        "Check out",
        "Pay now",
        "Purchase",
        "Transfer funds",
        "Sell",
        "Donate",
        "btnPay",
        "confirm_transaction",
        "submitPayment",
    ] {
        assert!(payment_label(text, "") && payment_label("", text), "{text}");
    }
    for text in [
        "交易记录",
        "查看转账记录",
        "支付历史",
        "购买历史",
        "PaymentHistory",
        "Transaction records",
        "order history",
        "play",
        "display",
        "Payroll",
        "保存",
        "发送消息",
        "删除文件",
    ] {
        assert!(!payment_label(text, ""), "{text}");
    }
    assert!(payment_label("交易记录：确认付款", ""));
    assert!(payment_label("Payment history", "payButton"));
}
