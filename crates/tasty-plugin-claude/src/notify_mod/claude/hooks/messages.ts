// These messages are visible in the conversation as well as read by the model.
const messages = {
  en: 'Tasty child status notifications (status changes are not proof of task success; inspect the results):',
  ko: 'Tasty 자식 상태 알림 (상태 변화는 작업 성공의 증거가 아닙니다. 결과를 확인하세요):',
  ja: 'Tasty 子セッションの状態通知（状態の変化は作業成功の証拠ではありません。結果を確認してください）:',
};
export function heading(locale: string | undefined): string {
  return messages[locale as keyof typeof messages] ?? messages.en;
}
