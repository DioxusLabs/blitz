package dev.dioxus.main

import android.app.NativeActivity
import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import androidx.core.view.ViewCompat
import androidx.core.view.OnApplyWindowInsetsListener
import androidx.core.view.WindowInsetsCompat

// Makes basic text input work with NativeActivity
class MainActivity : NativeActivity() { // ,OnApplyWindowInsetsListener {
    private fun getNativeActivityView(): View {
        // This is hacky as hell, but NativeActivity does not give any proper way of accessing it.
        var parent = window.decorView as ViewGroup
        parent = parent.getChildAt(0) as ViewGroup
        parent = parent.getChildAt(1) as ViewGroup
        return parent.getChildAt(0)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val nativeActivityView = getNativeActivityView()
        nativeActivityView.isFocusable = true
        nativeActivityView.isFocusableInTouchMode = true
        nativeActivityView.requestFocus()

        // ViewCompat.setOnApplyWindowInsetsListener(nativeActivityView, this)
    }
}