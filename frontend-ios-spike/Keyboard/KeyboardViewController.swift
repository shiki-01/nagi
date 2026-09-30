import UIKit
import SwiftUI

class KeyboardViewController: UIInputViewController {
    private let store = SharedStore.shared
    private var containerView: UIView?
    private var swiftUIView: KeyboardSwiftUIView?

    override func viewDidLoad() {
        super.viewDidLoad()

        setupUI()
    }

    private func setupUI() {
        // SwiftUI ホストをコンテナとして使う
        swiftUIView = KeyboardSwiftUIView(
            store: store,
            insertTextHandler: { [weak self] text in
                self?.textDocumentProxy.insertText(text)
            },
            deleteBackwardHandler: { [weak self] in
                self?.textDocumentProxy.deleteBackward()
            },
            advanceToNextInputModeHandler: { [weak self] in
                self?.advanceToNextInputMode()
            }
        )

        let hostView = UIHostingController(rootView: swiftUIView!)
        addChild(hostView)

        let containerView = UIView()
        self.view.addSubview(containerView)
        containerView.translatesAutoresizingMaskIntoConstraints = false

        NSLayoutConstraint.activate([
            containerView.leadingAnchor.constraint(equalTo: self.view.leadingAnchor),
            containerView.trailingAnchor.constraint(equalTo: self.view.trailingAnchor),
            containerView.topAnchor.constraint(equalTo: self.view.topAnchor),
            containerView.bottomAnchor.constraint(equalTo: self.view.bottomAnchor)
        ])

        self.view.addSubview(hostView.view)
        hostView.view.translatesAutoresizingMaskIntoConstraints = false

        NSLayoutConstraint.activate([
            hostView.view.leadingAnchor.constraint(equalTo: self.view.leadingAnchor),
            hostView.view.trailingAnchor.constraint(equalTo: self.view.trailingAnchor),
            hostView.view.topAnchor.constraint(equalTo: self.view.topAnchor),
            hostView.view.bottomAnchor.constraint(equalTo: self.view.bottomAnchor)
        ])

        hostView.didMove(toParent: self)
        self.containerView = containerView
    }
}

// MARK: - SwiftUI 部分

struct KeyboardSwiftUIView: View {
    let store: SharedStore
    let insertTextHandler: (String) -> Void
    let deleteBackwardHandler: () -> Void
    let advanceToNextInputModeHandler: () -> Void

    @State private var results: [ProbeResult] = []
    @State private var selectedProbe: String?
    @State private var isRunning = false

    var body: some View {
        VStack(spacing: 0) {
            // ヘッダ
            VStack {
                Text("Nagi Keyboard Probe")
                    .font(.headline)
                Text("各ボタンで検証項目を実行")
                    .font(.caption)
                    .foregroundColor(.gray)
            }
            .frame(maxWidth: .infinity)
            .padding()
            .background(Color(.systemGray6))

            // Probe ボタン
            ScrollView {
                VStack(spacing: 8) {
                    ForEach(Array(allProbes.keys.sorted()), id: \.self) { probeId in
                        Button(action: { runProbe(probeId) }) {
                            HStack {
                                Text(probeId)
                                Spacer()
                                if let latest = results.first(where: { $0.id == probeId }) {
                                    Image(systemName: latest.ok ? "checkmark.circle" : "xmark.circle")
                                        .foregroundColor(latest.ok ? .green : .red)
                                }
                            }
                            .frame(maxWidth: .infinity)
                            .padding()
                            .background(Color(.systemGray5))
                            .foregroundColor(.black)
                            .cornerRadius(8)
                        }
                        .disabled(isRunning)
                    }

                    Button(action: { runAllProbes() }) {
                        Text("すべて実行")
                            .frame(maxWidth: .infinity)
                            .padding()
                            .background(Color.blue)
                            .foregroundColor(.white)
                            .cornerRadius(8)
                    }
                    .disabled(isRunning)
                }
                .padding()
            }

            Divider()

            // テスト入力ボタン
            HStack(spacing: 4) {
                Button("あ") { insertTextHandler("あ") }
                    .frame(maxWidth: .infinity)
                    .padding(8)
                    .background(Color(.systemGray5))
                    .cornerRadius(4)

                Button("か") { insertTextHandler("か") }
                    .frame(maxWidth: .infinity)
                    .padding(8)
                    .background(Color(.systemGray5))
                    .cornerRadius(4)

                Button("Space") { insertTextHandler(" ") }
                    .frame(maxWidth: .infinity)
                    .padding(8)
                    .background(Color(.systemGray5))
                    .cornerRadius(4)

                Button("⌫") { deleteBackwardHandler() }
                    .frame(maxWidth: .infinity)
                    .padding(8)
                    .background(Color(.systemGray5))
                    .cornerRadius(4)

                Button("Next") { advanceToNextInputModeHandler() }
                    .frame(maxWidth: .infinity)
                    .padding(8)
                    .background(Color(.systemGray5))
                    .cornerRadius(4)
            }
            .padding()
        }
        .onAppear { loadResults() }
    }

    private func runProbe(_ probeId: String) {
        isRunning = true
        Task {
            if let probeRunner = allProbes[probeId] {
                let context = ProbeContext(
                    store: store,
                    processType: "keyboard",
                    engine: nil
                )
                let result = await probeRunner(context)
                try? store.append(result: result)
                loadResults()
            }
            isRunning = false
        }
    }

    private func runAllProbes() {
        isRunning = true
        Task {
            for (probeId, probeRunner) in allProbes {
                let context = ProbeContext(
                    store: store,
                    processType: "keyboard",
                    engine: nil
                )
                let result = await probeRunner(context)
                try? store.append(result: result)
            }
            loadResults()
            isRunning = false
        }
    }

    private func loadResults() {
        results = store.read(last: 100).filter { $0.process == "keyboard" }
    }
}

#Preview {
    KeyboardSwiftUIView(
        store: SharedStore.shared,
        insertTextHandler: { _ in },
        deleteBackwardHandler: { },
        advanceToNextInputModeHandler: { }
    )
}
