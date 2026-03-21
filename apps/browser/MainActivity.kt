package dev.dioxus.main

import android.app.NativeActivity
import android.os.Bundle
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup

class MainActivity : NativeActivity() {

    // one should probably traverse the view tree to find the rendering surface. this will work regardless of manufacturer.
    // example: if oem wraps those three previous getChildAt will not work. 
    // the only thing that matters here is surfaceview. 
    private fun findNativeSurfaceView(view: View): View? {
        if (view is SurfaceView) return view
        
        if (view is ViewGroup) {
            for (i in 0 until view.childCount) {
                val found = findNativeSurfaceView(view.getChildAt(i))
                if (found != null) return found
            }
        }
        return null
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        
        // wait until android has actually finished building the UI layout before search.
        window.decorView.post {
            val nativeView = findNativeSurfaceView(window.decorView)
            
            
            nativeView?.apply {
                isFocusable = true
                isFocusableInTouchMode = true
                requestFocus()
            }
        }
    }
}
