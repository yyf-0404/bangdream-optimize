// Keep destinations on validation errors so views need not parse translated messages.
export function validationError(message, target, page = 'activity') {
  const error = message instanceof Error ? message : new Error(message);
  error.validationTarget ??= {selector: target, page};
  return error;
}

export function validateAt(target, callback, page = 'activity') {
  try { return callback(); }
  catch (error) { throw validationError(error, target, page); }
}
