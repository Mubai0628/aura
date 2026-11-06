/** A call on a program of the user's computer: what each answer covers. The program itself is
 * named on the card. */
export function ComputerApprovalFacts() {
  return (
    <section aria-label="电脑操控">
      <p>
        「本会话允许」覆盖这个程序在本会话里的读取和操作，「总是允许」把它记进 设置 → 电脑操控
        的名单，以后的会话也不再问；停止后再操作会重新询问。
      </p>
    </section>
  );
}
