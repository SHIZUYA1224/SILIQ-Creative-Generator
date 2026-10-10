import type { Issue } from './api';

const guidance:Record<string,[string,string]>={
  E_FONT_MISSING:['使用フォントを読み込めません。','プロジェクト設定で同じフォントを再登録してください。'],
  E_GLYPH_MISSING:['フォントに含まれない文字があります。','文字を修正するか、必要な文字を含むフォントを選んでください。'],
  E_TEXT_OVERFLOW:['文字が枠に収まりません。','文字数・枠・行数を調整するか、許可する文字サイズを設定してください。'],
  E_VARIANT_NOT_APPROVED:['レイアウトが未承認です。','制作画面で内容を確認し、アートボードを承認してください。'],
  E_ASSET_MISSING:['必要な素材を読み込めません。','素材を登録し直して、使用する画像やロゴを選んでください。'],
  E_OUTPUT_COLLISION:['出力名が重複しています。','既存ファイルと重複しない名前や保存先を選んでください。'],
  E_DATA_INVALID:['入力データを使用できません。','列の対応付け、必須項目、行ID、文字数を確認してください。'],
  E_EDIT_FORBIDDEN:['この編集は許可されていません。','親を含むロック、制作モード、グループの変形条件を確認してください。'],
  E_RESOURCE_LIMIT:['作品が扱える容量を超えています。','素材の解像度、レイヤー数、出力サイズを減らしてください。'],
  E_PATH_DENIED:['選択した場所にアクセスできません。','このMac内の通常のファイルやフォルダを選び直してください。'],
  E_SAVE_FAILED:['保存または復旧データを読み書きできません。','空き容量と保存先を確認してください。現在の作品を別名で保存できます。'],
  E_PACKAGE_INVALID:['プロジェクトファイルを開けません。','元の保存データまたは復旧データを使用してください。'],
  E_REVISION_CONFLICT:['最新の編集内容と一致しません。','画面が更新されたら操作をやり直してください。'],
  E_LOGO_CLEARSPACE:['ロゴの余白に他の要素が重なっています。','要素を移動して、指定したロゴの余白を確保してください。'],
  E_LOGO_MIN_WIDTH:['ロゴのサイズが使用条件を満たしません。','ロゴを大きくし、縦横比を保ってください。'],
  E_EXPORT_UNSUPPORTED:['この書き出し設定は使用できません。','形式・背景色・解像度・画像化への許可を確認してください。'],
  W_IMAGE_UPSCALED:['画像を元の解像度より拡大しています。','鮮明さが必要な場合は、より大きな画像を使用してください。'],
};
export function IssueDetails({issue}:{issue:Issue}) {
  const text=guidance[issue.code];
  return <><p>{text?.[0] ?? issue.message}<small>{issue.location.rowId && `行 ${issue.location.rowId} · `}{issue.location.nodeId && `レイヤー ${issue.location.nodeId} · `}{text?.[1] ?? issue.remedy}</small></p>{text && <details className="issue-diagnostic"><summary>詳細</summary><p>{issue.message}<small>{issue.remedy}</small></p></details>}</>;
}
