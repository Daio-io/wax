import React, { forwardRef, memo } from "react";

export function Button() {
  return <button />;
}

export const Card = () => <section />;

function ButtonBase() {
  return <button data-base />;
}

export const MemoButton = memo(ButtonBase);

export const TextInput = forwardRef(function TextInput() {
  return <input />;
});

export const InlineMemo = React.memo(() => <span />);

export const InlineRef = React.forwardRef((props, ref) => <input ref={ref} />);

const wrapperFactory = {
  memo(component) {
    return component;
  },
  forwardRef(component) {
    return component;
  },
};

export const FactoryMemo = wrapperFactory.memo(() => <div />);

export const FactoryRef = wrapperFactory.forwardRef(() => <div />);

export class Dialog extends React.Component {
  render() {
    return <div role="dialog" />;
  }
}

function PrivateBadge() {
  return <span />;
}

export const lowerBadge = () => <span />;

export const notComponent = 42;

export default function DefaultPanel() {
  return <aside />;
}
