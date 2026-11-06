//! Financial action labels shared by desktop admission and delivery checks.

#[must_use]
pub fn payment_label(name: &str, automation_id: &str) -> bool {
    [name, automation_id].into_iter().any(financial_words)
}

fn financial_words(text: &str) -> bool {
    let mut words = String::new();
    let mut previous_lower = false;
    for character in text.chars() {
        if character.is_ascii_uppercase() && previous_lower {
            words.push(' ');
        }
        previous_lower = character.is_ascii_lowercase();
        words.push(if character.is_ascii_alphanumeric() || !character.is_ascii() {
            character.to_ascii_lowercase()
        } else {
            ' '
        });
    }
    for view in [
        "交易记录",
        "交易历史",
        "交易查询",
        "付款记录",
        "付款历史",
        "支付记录",
        "支付历史",
        "转账记录",
        "转账历史",
        "购买记录",
        "购买历史",
        "订单记录",
        "订单历史",
        "订单详情",
        "transaction history",
        "transaction records",
        "payment history",
        "payment records",
        "transfer history",
        "transfer records",
        "purchase history",
        "order history",
        "order details",
    ] {
        words = words.replace(view, " ");
    }
    if [
        "付款", "支付", "转账", "汇款", "交易", "购买", "下单", "结算", "提现", "捐赠", "订阅",
        "买入", "卖出",
    ]
    .iter()
    .any(|word| words.contains(word))
    {
        return true;
    }
    words.split_whitespace().any(|word| {
        matches!(
            word,
            "buy"
                | "purchase"
                | "order"
                | "checkout"
                | "pay"
                | "payment"
                | "transfer"
                | "remit"
                | "withdraw"
                | "donate"
                | "subscribe"
                | "trade"
                | "transaction"
                | "sell"
                | "repay"
        )
    }) || words.contains("check out")
}
