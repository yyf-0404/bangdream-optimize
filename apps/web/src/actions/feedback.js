const MAX_ATTACHMENT_COUNT = 3;
const MAX_ATTACHMENT_BYTES = 5 * 1024 * 1024;
const MAX_TOTAL_ATTACHMENT_BYTES = 10 * 1024 * 1024;
const ALLOWED_ATTACHMENT = /\.(?:png|jpe?g|gif|webp|txt|log|json|zip|pdf)$/i;

export function createFeedbackActions({
  state,
  elements,
  diagnosticFileName,
}) {
  let includeDiagnostic = false;

  function handleOpenFeedback() {
    includeDiagnostic = false;
    openFeedbackDialog();
  }

  function handleOpenResultFeedback() {
    includeDiagnostic = state.lastDiagnostic != null;
    if (!elements.feedbackSubject.value.trim()) {
      elements.feedbackSubject.value = '计算结果反馈';
    }
    elements.feedbackCategory.value = 'problem';
    openFeedbackDialog();
  }

  function openFeedbackDialog() {
    elements.feedbackStatus.textContent = '';
    elements.feedbackStatus.className = 'feedback-status';
    renderAttachmentSummary();
    if (typeof elements.feedbackDialog.showModal === 'function') {
      elements.feedbackDialog.showModal();
    } else {
      elements.feedbackDialog.setAttribute('open', '');
    }
    elements.feedbackCategory.focus();
  }

  function handleCloseFeedback() {
    elements.feedbackDialog.close();
  }

  function handleFeedbackAttachmentsChange() {
    renderAttachmentSummary();
  }

  async function handleSubmitFeedback(event) {
    event.preventDefault();
    if (!elements.feedbackForm.reportValidity()) {
      return;
    }
    const button = elements.submitFeedback;
    button.disabled = true;
    button.classList.add('is-loading');
    elements.feedbackStatus.textContent = '正在发送反馈…';
    elements.feedbackStatus.className = 'feedback-status';
    try {
      const attachments = buildAttachments();
      const activePage = Array.from(elements.pageTabs)
        .find((tab) => tab.classList.contains('active'))
        ?.dataset.page ?? 'unknown';
      const result = await state.runtime.submitFeedback(
        {
          category: elements.feedbackCategory.value,
          contactEmail: emptyToNull(elements.feedbackContactEmail.value),
          subject: elements.feedbackSubject.value.trim(),
          content: elements.feedbackContent.value.trim(),
          website: elements.feedbackWebsite.value,
          context: {
            runtime: state.runtime.kind ?? 'unknown',
            appVersion: elements.appVersion.textContent.replace(/^v/, ''),
            page: activePage,
          },
        },
        attachments,
      );
      elements.feedbackForm.reset();
      includeDiagnostic = false;
      renderAttachmentSummary();
      elements.feedbackStatus.textContent = `反馈已发送，编号：${result.feedbackId}`;
      elements.feedbackStatus.className = 'feedback-status success';
    } catch (error) {
      elements.feedbackStatus.textContent = `发送失败：${error?.message ?? String(error)}`;
      elements.feedbackStatus.className = 'feedback-status error';
    } finally {
      button.disabled = false;
      button.classList.remove('is-loading');
    }
  }

  function buildAttachments() {
    const attachments = Array.from(elements.feedbackAttachments.files ?? []);
    if (includeDiagnostic && state.lastDiagnostic) {
      const json = JSON.stringify(state.lastDiagnostic, null, 2);
      attachments.push(new File(
        [json],
        diagnosticFileName(state.lastDiagnostic),
        { type: 'application/json' },
      ));
    }
    validateAttachments(attachments);
    return attachments;
  }

  function validateAttachments(attachments) {
    if (attachments.length > MAX_ATTACHMENT_COUNT) {
      throw new Error(`最多只能上传 ${MAX_ATTACHMENT_COUNT} 个附件（自动诊断也计入）`);
    }
    let totalBytes = 0;
    for (const attachment of attachments) {
      if (!ALLOWED_ATTACHMENT.test(attachment.name)) {
        throw new Error(`不支持的附件类型：${attachment.name}`);
      }
      if (attachment.size === 0) {
        throw new Error(`附件不能为空：${attachment.name}`);
      }
      if (attachment.size > MAX_ATTACHMENT_BYTES) {
        throw new Error(`单个附件不能超过 5 MiB：${attachment.name}`);
      }
      totalBytes += attachment.size;
    }
    if (totalBytes > MAX_TOTAL_ATTACHMENT_BYTES) {
      throw new Error('附件总大小不能超过 10 MiB');
    }
  }

  function renderAttachmentSummary() {
    const files = Array.from(elements.feedbackAttachments.files ?? []);
    const parts = files.map((file) => `${file.name}（${formatBytes(file.size)}）`);
    if (includeDiagnostic && state.lastDiagnostic) {
      parts.push(`自动诊断：${diagnosticFileName(state.lastDiagnostic)}`);
    } else if (includeDiagnostic) {
      parts.push('当前没有可用的诊断数据');
    }
    elements.feedbackAttachmentSummary.textContent = parts.length > 0
      ? parts.join('；')
      : '未选择附件';
    elements.feedbackDiagnosticNotice.hidden =
      !(includeDiagnostic && state.lastDiagnostic);
  }

  return {
    handleCloseFeedback,
    handleFeedbackAttachmentsChange,
    handleOpenFeedback,
    handleOpenResultFeedback,
    handleSubmitFeedback,
  };
}

function emptyToNull(value) {
  const normalized = String(value ?? '').trim();
  return normalized || null;
}

function formatBytes(bytes) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KiB`;
  }
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}
