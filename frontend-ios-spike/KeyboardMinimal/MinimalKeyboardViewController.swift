import UIKit

// 切り分け実験用の最小キーボード拡張（project-minimal-keyboard.yml でのみビルドされる）。
// NagiCore / Shared に依存しない。UIKit のみ。
final class MinimalKeyboardViewController: UIInputViewController {
    override func viewDidLoad() {
        super.viewDidLoad()

        let label = UILabel()
        label.text = "Nagi 最小キーボード"
        label.textAlignment = .center

        let nextButton = UIButton(type: .system)
        nextButton.setTitle("次のキーボード", for: .normal)
        nextButton.addTarget(self, action: #selector(handleInputModeList(from:with:)), for: .allTouchEvents)

        let insertButton = UIButton(type: .system)
        insertButton.setTitle("あ", for: .normal)
        insertButton.addTarget(self, action: #selector(insertA), for: .touchUpInside)

        let buttons = UIStackView(arrangedSubviews: [nextButton, insertButton])
        buttons.axis = .horizontal
        buttons.distribution = .fillEqually

        let stack = UIStackView(arrangedSubviews: [label, buttons])
        stack.axis = .vertical
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 8),
            stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -8),
            stack.topAnchor.constraint(equalTo: view.topAnchor, constant: 8),
            stack.bottomAnchor.constraint(equalTo: view.bottomAnchor, constant: -8),
            view.heightAnchor.constraint(greaterThanOrEqualToConstant: 160),
        ])
    }

    @objc private func insertA() {
        textDocumentProxy.insertText("あ")
    }
}
