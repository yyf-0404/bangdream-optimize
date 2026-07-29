import assert from 'node:assert/strict';
import test from 'node:test';

import { createFeedbackActions } from '../src/actions/feedback.js';

function createHarness() {
  const submissions = [];
  const classNames = new Set();
  const elements = {
    appVersion: { textContent: 'v0.3.3' },
    feedbackAttachmentSummary: { textContent: '' },
    feedbackAttachments: { files: [] },
    feedbackCategory: { value: 'suggestion', focus() {} },
    feedbackContactEmail: { value: '' },
    feedbackContent: { value: '计算结果与预期不一致' },
    feedbackDiagnosticNotice: { hidden: true },
    feedbackDialog: { showModal() {} },
    feedbackForm: {
      reportValidity: () => true,
      reset() {},
    },
    feedbackStatus: { textContent: '', className: '' },
    feedbackSubject: { value: '' },
    feedbackWebsite: { value: '' },
    pageTabs: [{
      dataset: { page: 'result' },
      classList: { contains: (name) => name === 'active' },
    }],
    submitFeedback: {
      disabled: false,
      classList: {
        add: (name) => classNames.add(name),
        remove: (name) => classNames.delete(name),
      },
    },
  };
  const state = {
    lastDiagnostic: { kind: 'success', result: { score: 123 } },
    runtime: {
      kind: 'browser',
      async submitFeedback(payload, attachments) {
        submissions.push({ payload, attachments });
        return { feedbackId: 'F-test' };
      },
    },
  };
  const actions = createFeedbackActions({
    state,
    elements,
    diagnosticFileName: () => 'calculation-diagnostic.json',
  });
  return { actions, elements, submissions };
}

test('result feedback automatically includes the current diagnostic', async () => {
  const { actions, elements, submissions } = createHarness();

  actions.handleOpenResultFeedback();
  assert.equal(elements.feedbackDiagnosticNotice.hidden, false);
  assert.match(elements.feedbackAttachmentSummary.textContent, /calculation-diagnostic\.json/);

  await actions.handleSubmitFeedback({ preventDefault() {} });
  assert.equal(submissions.length, 1);
  assert.equal(submissions[0].attachments.length, 1);
  assert.equal(submissions[0].attachments[0].name, 'calculation-diagnostic.json');
  assert.deepEqual(
    JSON.parse(await submissions[0].attachments[0].text()),
    { kind: 'success', result: { score: 123 } },
  );
});

test('sidebar feedback does not implicitly include diagnostics', async () => {
  const { actions, elements, submissions } = createHarness();

  actions.handleOpenFeedback();
  assert.equal(elements.feedbackDiagnosticNotice.hidden, true);

  await actions.handleSubmitFeedback({ preventDefault() {} });
  assert.equal(submissions.length, 1);
  assert.deepEqual(submissions[0].attachments, []);
});
